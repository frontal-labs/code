//! Worker tokens, stored in Postgres.
//!
//! A worker's token is the only credential it holds, so it must outlive a server
//! restart and must be revocable. It is kept in the `hosted_worker_tokens` table as a
//! SHA-256 hash, never in plaintext, and never in server memory.
//!
//! The Postgres connection is verified TLS. The server checks the database certificate
//! against the system roots, plus an optional CA file for private deployments. A plaintext
//! connection is allowed only to a loopback database, and only when the URL asks for
//! `sslmode=disable`.
//!
//! The Postgres client lives on a dedicated thread with its own runtime. That lets the
//! synchronous lane transport and the asynchronous HTTP handlers use one store without
//! blocking a runtime. Sync callers wait on a std channel. Async callers await a oneshot.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc as std_mpsc;

use rustls_pki_types::pem::PemObject;
use rustls_pki_types::CertificateDer;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::{Client, NoTls, Socket};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS hosted_worker_tokens (
    token_hash TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL,
    issued_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS hosted_worker_tokens_task_idx
    ON hosted_worker_tokens (task_id) WHERE revoked_at IS NULL;
";

/// How the store connects to Postgres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DatabaseTls {
    /// TLS with the database certificate verified against the system roots and, when
    /// given, an extra CA file.
    Verified { extra_ca_file: Option<PathBuf> },
    /// No TLS. Only for a loopback database whose URL says `sslmode=disable`.
    Plaintext,
}

/// Decides how to connect from the URL and the configured CA file. Refuses plaintext to
/// anything that is not loopback, and refuses an unrecognized `sslmode`.
pub(crate) fn tls_mode_for(
    url: &str,
    extra_ca_file: Option<PathBuf>,
) -> Result<DatabaseTls, String> {
    let sslmode = sslmode_of(url);
    let config = tokio_postgres::Config::from_str(&strip_sslmode(url))
        .map_err(|error| format!("invalid database URL: {error}"))?;
    let loopback = config.get_hosts().iter().all(|host| match host {
        tokio_postgres::config::Host::Tcp(name) => {
            matches!(name.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
        }
        tokio_postgres::config::Host::Unix(_) => true,
    });
    match sslmode.as_deref() {
        Some("disable") if loopback => Ok(DatabaseTls::Plaintext),
        Some("disable") => Err(
            "refusing a plaintext connection to a non-local Postgres host; remove sslmode=disable"
                .to_string(),
        ),
        None | Some("verify-full" | "verify-ca" | "require" | "prefer") => {
            Ok(DatabaseTls::Verified { extra_ca_file })
        }
        Some(other) => Err(format!("unsupported sslmode `{other}` for the token store")),
    }
}

/// The `sslmode` query parameter. `tokio_postgres` rejects the verification modes, so the
/// server reads the mode itself.
fn sslmode_of(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("sslmode="))
        .map(str::to_string)
}

/// The URL without its `sslmode` parameter, for the driver.
fn strip_sslmode(url: &str) -> String {
    match url.split_once('?') {
        None => url.to_string(),
        Some((base, query)) => {
            let rest: Vec<&str> = query
                .split('&')
                .filter(|pair| !pair.starts_with("sslmode="))
                .collect();
            if rest.is_empty() {
                base.to_string()
            } else {
                format!("{base}?{}", rest.join("&"))
            }
        }
    }
}

/// A rustls client configuration that verifies the server against the system roots and an
/// optional extra CA file.
fn rustls_client_config(extra_ca_file: Option<&Path>) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = extra_ca_file {
        let certificates = CertificateDer::pem_file_iter(path)
            .map_err(|error| format!("reading the database CA file: {error}"))?;
        for certificate in certificates {
            let certificate =
                certificate.map_err(|error| format!("parsing the database CA file: {error}"))?;
            roots
                .add(certificate)
                .map_err(|error| format!("adding the database CA: {error}"))?;
        }
    }
    let builder = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| format!("TLS setup: {error}"))?;
    Ok(builder.with_root_certificates(roots).with_no_client_auth())
}

/// Opens one connection and drives it on a background task.
pub(crate) async fn connect_client(url: &str, tls: &DatabaseTls) -> Result<Client, String> {
    let config = tokio_postgres::Config::from_str(&strip_sslmode(url))
        .map_err(|error| format!("invalid database URL: {error}"))?;
    match tls {
        DatabaseTls::Plaintext => {
            let (client, connection) = config
                .connect(NoTls)
                .await
                .map_err(|error| format!("failed to connect to Postgres: {error}"))?;
            spawn_connection(connection);
            Ok(client)
        }
        DatabaseTls::Verified { extra_ca_file } => {
            let mut config = config;
            config.ssl_mode(tokio_postgres::config::SslMode::Require);
            let connector = tokio_postgres_rustls::MakeRustlsConnect::new(rustls_client_config(
                extra_ca_file.as_deref(),
            )?);
            let (client, connection) = config.connect(connector).await.map_err(|error| {
                format!(
                    "failed to connect to Postgres over TLS: {}",
                    error_chain(&error)
                )
            })?;
            spawn_connection(connection);
            Ok(client)
        }
    }
}

fn spawn_connection<S>(connection: tokio_postgres::Connection<Socket, S>)
where
    S: tokio_postgres::tls::TlsStream + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        // The task ends when the client is dropped or the socket fails.
        let _ = connection.await;
    });
}

/// The error and everything beneath it. The driver's top-level message says only that the
/// handshake failed, and the cause is in the source chain.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

pub(crate) fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// A reply channel for either a blocking or an async caller.
enum Responder<T> {
    Blocking(std_mpsc::Sender<T>),
    Async(oneshot::Sender<T>),
}

impl<T> Responder<T> {
    fn send(self, value: T) {
        match self {
            Self::Blocking(sender) => {
                let _ = sender.send(value);
            }
            Self::Async(sender) => {
                let _ = sender.send(value);
            }
        }
    }
}

enum Command {
    Issue {
        task_id: String,
        ttl_secs: f64,
        reply: Responder<Result<String, String>>,
    },
    Validate {
        token: String,
        reply: Responder<Result<Option<String>, String>>,
    },
    Revoke {
        task_id: String,
        reply: Responder<Result<u64, String>>,
    },
}

/// Handle to the token store. Cheap to clone; every clone talks to the same thread.
#[derive(Clone)]
pub(crate) struct WorkerTokenStore {
    commands: mpsc::UnboundedSender<Command>,
}

impl std::fmt::Debug for WorkerTokenStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WorkerTokenStore(postgres)")
    }
}

impl WorkerTokenStore {
    /// Connects, creates the schema if needed, and starts the store thread. Returns once the
    /// first connection and migration have succeeded.
    pub(crate) fn connect(url: &str, tls: DatabaseTls) -> Result<Self, String> {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let url = url.to_string();
        std::thread::Builder::new()
            .name("worker-token-store".to_string())
            .spawn(move || run_store(url, tls, commands_rx, ready_tx))
            .map_err(|error| format!("failed to start the token store thread: {error}"))?;
        ready_rx
            .recv()
            .map_err(|_| "token store thread exited before it was ready".to_string())??;
        Ok(Self {
            commands: commands_tx,
        })
    }

    /// Issues a token for `task_id` and revokes the task's previous token. For the
    /// synchronous lane transport.
    pub(crate) fn issue_blocking(&self, task_id: &str, ttl_secs: u64) -> Result<String, String> {
        let (reply_tx, reply_rx) = std_mpsc::channel();
        // Lifetimes beyond u32::MAX seconds (136 years) are clamped, not wrapped.
        let ttl_secs = f64::from(u32::try_from(ttl_secs).unwrap_or(u32::MAX));
        self.send(Command::Issue {
            task_id: task_id.to_string(),
            ttl_secs,
            reply: Responder::Blocking(reply_tx),
        })?;
        reply_rx
            .recv()
            .map_err(|_| "token store did not answer".to_string())?
    }

    /// Returns the task a live token belongs to. `Ok(None)` means the token is unknown,
    /// expired, or revoked.
    pub(crate) async fn validate(&self, token: &str) -> Result<Option<String>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(Command::Validate {
            token: token.to_string(),
            reply: Responder::Async(reply_tx),
        })?;
        reply_rx
            .await
            .map_err(|_| "token store did not answer".to_string())?
    }

    /// Revokes every live token for `task_id`. For async callers.
    pub(crate) async fn revoke(&self, task_id: &str) -> Result<u64, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(Command::Revoke {
            task_id: task_id.to_string(),
            reply: Responder::Async(reply_tx),
        })?;
        reply_rx
            .await
            .map_err(|_| "token store did not answer".to_string())?
    }

    /// Revokes every live token for `task_id`. For synchronous callers.
    pub(crate) fn revoke_blocking(&self, task_id: &str) -> Result<u64, String> {
        let (reply_tx, reply_rx) = std_mpsc::channel();
        self.send(Command::Revoke {
            task_id: task_id.to_string(),
            reply: Responder::Blocking(reply_tx),
        })?;
        reply_rx
            .recv()
            .map_err(|_| "token store did not answer".to_string())?
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "token store thread has stopped".to_string())
    }
}

fn run_store(
    url: String,
    tls: DatabaseTls,
    mut commands: mpsc::UnboundedReceiver<Command>,
    ready: std_mpsc::Sender<Result<(), String>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(format!("token store runtime: {error}")));
            return;
        }
    };
    runtime.block_on(async move {
        let mut client = match connect(&url, &tls).await {
            Ok(client) => {
                let _ = ready.send(Ok(()));
                Some(client)
            }
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        while let Some(command) = commands.recv().await {
            // Reconnect when the connection dropped (database restart, network blip).
            if client.as_ref().is_none_or(Client::is_closed) {
                client = connect(&url, &tls).await.ok();
            }
            let Some(db) = client.as_mut() else {
                reply_unavailable(command);
                continue;
            };
            match command {
                Command::Issue {
                    task_id,
                    ttl_secs,
                    reply,
                } => reply.send(issue(db, &task_id, ttl_secs).await),
                Command::Validate { token, reply } => reply.send(validate(db, &token).await),
                Command::Revoke { task_id, reply } => reply.send(revoke(db, &task_id).await),
            }
        }
    });
}

async fn connect(url: &str, tls: &DatabaseTls) -> Result<Client, String> {
    let client = connect_client(url, tls).await?;
    client
        .batch_execute(SCHEMA)
        .await
        .map_err(|error| format!("failed to create the worker token schema: {error}"))?;
    Ok(client)
}

fn reply_unavailable(command: Command) {
    let message = "token store is unavailable: Postgres is not reachable".to_string();
    match command {
        Command::Issue { reply, .. } => reply.send(Err(message)),
        Command::Validate { reply, .. } => reply.send(Err(message)),
        Command::Revoke { reply, .. } => reply.send(Err(message)),
    }
}

async fn issue(db: &mut Client, task_id: &str, ttl_secs: f64) -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("random source: {error}"))?;
    let token = hex::encode(bytes);
    let hash = hash_token(&token);

    let transaction = db
        .transaction()
        .await
        .map_err(|error| format!("token store: {error}"))?;
    transaction
        .execute(
            "UPDATE hosted_worker_tokens SET revoked_at = now()
             WHERE task_id = $1 AND revoked_at IS NULL",
            &[&task_id],
        )
        .await
        .map_err(|error| format!("token store: {error}"))?;
    transaction
        .execute(
            "INSERT INTO hosted_worker_tokens (token_hash, task_id, expires_at)
             VALUES ($1, $2, now() + make_interval(secs => $3))",
            &[&hash, &task_id, &ttl_secs],
        )
        .await
        .map_err(|error| format!("token store: {error}"))?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("token store: {error}"))?;
    Ok(token)
}

async fn validate(db: &mut Client, token: &str) -> Result<Option<String>, String> {
    let hash = hash_token(token);
    let row = db
        .query_opt(
            "SELECT task_id FROM hosted_worker_tokens
             WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
            &[&hash],
        )
        .await
        .map_err(|error| format!("token store: {error}"))?;
    Ok(row.map(|row| row.get::<_, String>(0)))
}

async fn revoke(db: &mut Client, task_id: &str) -> Result<u64, String> {
    db.execute(
        "UPDATE hosted_worker_tokens SET revoked_at = now()
         WHERE task_id = $1 AND revoked_at IS NULL",
        &[&task_id],
    )
    .await
    .map_err(|error| format!("token store: {error}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn plaintext_is_allowed_only_for_loopback_and_only_when_asked() {
        assert_eq!(
            tls_mode_for("postgres://u:p@127.0.0.1:5432/db?sslmode=disable", None),
            Ok(DatabaseTls::Plaintext)
        );
        assert!(tls_mode_for("postgres://u:p@db.internal/db?sslmode=disable", None).is_err());
        // Without an explicit disable, a database is reached over verified TLS, loopback or not.
        assert_eq!(
            tls_mode_for("postgres://u:p@127.0.0.1:5432/db", None),
            Ok(DatabaseTls::Verified {
                extra_ca_file: None
            })
        );
        assert!(tls_mode_for("postgres://u:p@db/db?sslmode=bogus", None).is_err());
        assert!(tls_mode_for("not a url", None).is_err());
    }

    #[test]
    fn sslmode_is_read_and_removed_before_the_driver_sees_the_url() {
        assert_eq!(
            strip_sslmode("postgres://u@h/db?sslmode=verify-full&application_name=x"),
            "postgres://u@h/db?application_name=x"
        );
        assert_eq!(
            strip_sslmode("postgres://u@h/db?sslmode=verify-full"),
            "postgres://u@h/db"
        );
    }

    #[test]
    fn token_hash_is_stable_and_not_the_token() {
        let hash = hash_token("abc");
        assert_eq!(hash, hash_token("abc"));
        assert_ne!(hash, "abc");
        assert_eq!(hash.len(), 64);
    }

    /// The database for tests, over verified TLS. `FRONTAL_TEST_POSTGRES_URL` names it, and
    /// `FRONTAL_TEST_POSTGRES_CA_FILE` names the CA that signed its certificate. Without the
    /// URL the database-backed tests return early, as the other live-service tests do.
    pub(crate) fn test_database() -> Option<(String, DatabaseTls)> {
        let url = std::env::var("FRONTAL_TEST_POSTGRES_URL").ok()?;
        let ca = std::env::var("FRONTAL_TEST_POSTGRES_CA_FILE")
            .ok()
            .map(PathBuf::from);
        // A URL that is set but unusable is a failed test, not a skipped one.
        let tls = tls_mode_for(&url, ca).expect("FRONTAL_TEST_POSTGRES_URL must be a usable URL");
        Some((url, tls))
    }

    pub(crate) fn test_store() -> Option<WorkerTokenStore> {
        let (url, tls) = test_database()?;
        Some(WorkerTokenStore::connect(&url, tls).expect(
            "FRONTAL_TEST_POSTGRES_URL is set but the database did not accept the connection",
        ))
    }

    /// Task ids are unique per test, so tests that share one database do not revoke each
    /// other's tokens.
    pub(crate) fn unique_task(prefix: &str) -> String {
        let mut bytes = [0_u8; 6];
        getrandom::getrandom(&mut bytes).expect("random");
        format!("{prefix}-{}", hex::encode(bytes))
    }

    #[test]
    fn issued_token_resolves_until_revoked_and_reissue_invalidates_the_old_one() {
        let Some(store) = test_store() else { return };
        let task = unique_task("revoke");
        let first = store.issue_blocking(&task, 3600).expect("first token");
        assert_eq!(
            futures_block(store.validate(&first)).expect("validate"),
            Some(task.clone())
        );

        let second = store.issue_blocking(&task, 3600).expect("second token");
        assert_eq!(
            futures_block(store.validate(&first)).expect("validate"),
            None
        );
        assert_eq!(
            futures_block(store.validate(&second)).expect("validate"),
            Some(task.clone())
        );

        store.revoke_blocking(&task).expect("revoke");
        assert_eq!(
            futures_block(store.validate(&second)).expect("validate"),
            None
        );
    }

    #[test]
    fn expired_tokens_are_refused() {
        let Some(store) = test_store() else { return };
        let task = unique_task("expire");
        let token = store.issue_blocking(&task, 0).expect("token");
        assert_eq!(
            futures_block(store.validate(&token)).expect("validate"),
            None
        );
    }

    #[test]
    fn tokens_are_stored_as_hashes_only() {
        let Some((url, tls)) = test_database() else {
            return;
        };
        let store = WorkerTokenStore::connect(&url, tls.clone()).expect("store");
        let task = unique_task("hash");
        let token = store.issue_blocking(&task, 3600).expect("token");
        let stored = futures_block(async {
            let client = connect_client(&url, &tls).await.expect("connect");
            client
                .query(
                    "SELECT token_hash FROM hosted_worker_tokens WHERE task_id = $1",
                    &[&task],
                )
                .await
                .expect("select")
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>()
        });
        assert_eq!(stored, vec![hash_token(&token)]);
        assert!(!stored.iter().any(|value| value == &token));
    }

    #[test]
    fn a_private_ca_is_refused_without_the_ca_file() {
        // Only meaningful when the test database uses a certificate from a private CA.
        let Some((url, _)) = test_database() else {
            return;
        };
        if std::env::var("FRONTAL_TEST_POSTGRES_CA_FILE").is_err() {
            return;
        }
        let refused = futures_block(connect_client(
            &url,
            &DatabaseTls::Verified {
                extra_ca_file: None,
            },
        ));
        assert!(refused.is_err(), "system roots must not trust a private CA");
    }

    fn futures_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
