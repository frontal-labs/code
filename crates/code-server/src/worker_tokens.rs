//! Worker tokens, stored in Postgres.
//!
//! A worker's token is the only credential it holds, so it must outlive a server
//! restart and must be revocable. It is kept in the `hosted_worker_tokens` table as a
//! SHA-256 hash, never in plaintext, and never in server memory.
//!
//! The Postgres client lives on a dedicated thread with its own runtime. That lets the
//! synchronous lane transport and the asynchronous HTTP handlers use one store without
//! blocking a runtime. Sync callers wait on a std channel. Async callers await a oneshot.

use std::str::FromStr;
use std::sync::mpsc as std_mpsc;

use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::{Client, NoTls};

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

/// Refuses a plaintext connection to a non-local database. Tokens and the database
/// password would cross the network unencrypted otherwise. The client has no TLS
/// connector yet, so a remote database needs an explicit opt-in on a private network.
pub(crate) fn check_database_url(url: &str, allow_plaintext: bool) -> Result<(), String> {
    let config = tokio_postgres::Config::from_str(url)
        .map_err(|error| format!("invalid database URL: {error}"))?;
    let local = config.get_hosts().iter().all(|host| match host {
        tokio_postgres::config::Host::Tcp(name) => {
            matches!(name.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
        }
        tokio_postgres::config::Host::Unix(_) => true,
    });
    if local || allow_plaintext {
        Ok(())
    } else {
        Err(
            "refusing a plaintext connection to a non-local Postgres host; \
             set FRONTAL_CODE_SERVER_DATABASE_ALLOW_PLAINTEXT=1 only on a private network"
                .to_string(),
        )
    }
}

pub(crate) fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

impl WorkerTokenStore {
    /// Connects, creates the schema if needed, and starts the store thread.
    /// Returns once the first connection and migration have succeeded.
    pub(crate) fn connect(url: &str, allow_plaintext: bool) -> Result<Self, String> {
        check_database_url(url, allow_plaintext)?;
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let url = url.to_string();
        std::thread::Builder::new()
            .name("worker-token-store".to_string())
            .spawn(move || run_store(url, commands_rx, ready_tx))
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
        self.send(Command::Issue {
            task_id: task_id.to_string(),
            ttl_secs: ttl_secs as f64,
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
        let mut client = match connect(&url).await {
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
                client = connect(&url).await.ok();
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

async fn connect(url: &str) -> Result<Client, String> {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(|error| format!("failed to connect to Postgres: {error}"))?;
    tokio::spawn(async move {
        // The connection task ends when the client is dropped or the socket fails.
        let _ = connection.await;
    });
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
    fn plaintext_is_allowed_only_for_local_databases_or_with_opt_in() {
        assert!(check_database_url("postgres://u:p@127.0.0.1:5432/db", false).is_ok());
        assert!(check_database_url("postgres://u:p@localhost/db", false).is_ok());
        assert!(check_database_url("postgres://u:p@db.internal/db", false).is_err());
        assert!(check_database_url("postgres://u:p@db.internal/db", true).is_ok());
        assert!(check_database_url("not a url", false).is_err());
    }

    #[test]
    fn token_hash_is_stable_and_not_the_token() {
        let hash = hash_token("abc");
        assert_eq!(hash, hash_token("abc"));
        assert_ne!(hash, "abc");
        assert_eq!(hash.len(), 64);
    }

    /// A store on the database named by `FRONTAL_TEST_POSTGRES_URL`. Without that
    /// variable the database-backed tests return early, as the other live-service tests do.
    pub(crate) fn test_store() -> Option<WorkerTokenStore> {
        let url = std::env::var("FRONTAL_TEST_POSTGRES_URL").ok()?;
        WorkerTokenStore::connect(&url, true).ok()
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
        let Some(store) = test_store() else { return };
        let task = unique_task("hash");
        let token = store.issue_blocking(&task, 3600).expect("token");
        let url = std::env::var("FRONTAL_TEST_POSTGRES_URL").expect("url");
        let stored = futures_block(async {
            let (client, connection) = tokio_postgres::connect(&url, NoTls).await.expect("connect");
            tokio::spawn(connection);
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

    fn futures_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
