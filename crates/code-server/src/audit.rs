//! Audit events for worker activity, written to `ClickHouse`.
//!
//! Events cover what an operator needs after an incident: when a worker token was issued
//! or revoked, when a worker started, and what was published and where. Events are
//! best effort. Emitting one never blocks a request, and a failed write is reported on
//! stderr. The store is not on the request path, so a `ClickHouse` outage cannot stop a
//! task.
//!
//! Event data never contains a token, a provider key, or a GitHub credential. Callers
//! pass identifiers and outcomes only.

use std::sync::mpsc as std_mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;

/// Versioned: a schema change gets a new table name, so an old table is never reused silently.
const TABLE: &str = "frontal_audit_events_v2";

/// Timestamps are stored as epoch milliseconds, which needs no date parsing on the way in.
/// Read them with `fromUnixTimestamp64Milli(ts_ms)`.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS frontal_audit_events_v2 (
    ts_ms    UInt64,
    event    LowCardinality(String),
    task_id  String,
    detail   String
) ENGINE = MergeTree ORDER BY (ts_ms, task_id)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClickHouseSettings {
    pub url: String,
    pub user: String,
    pub password: String,
}

/// Checks that the endpoint is either HTTPS, or HTTP to loopback. Credentials are sent with
/// every request, so cleartext is allowed only where traffic cannot leave the machine.
pub(crate) fn check_endpoint(url: &str) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|error| format!("invalid ClickHouse URL: {error}"))?;
    let host = parsed.host_str().unwrap_or_default();
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if loopback => Ok(()),
        "http" => Err(
            "refusing cleartext ClickHouse credentials to a non-local host; use https".to_string(),
        ),
        other => Err(format!("unsupported ClickHouse URL scheme `{other}`")),
    }
}

#[derive(Debug)]
struct Event {
    name: &'static str,
    task_id: String,
    detail: String,
    ts_millis: u64,
}

/// Handle for writing audit events. Cheap to clone.
#[derive(Clone, Debug)]
pub(crate) struct AuditSink {
    events: mpsc::UnboundedSender<Event>,
}

impl AuditSink {
    /// Creates the table if needed and starts the writer thread. Fails when the endpoint is
    /// unreachable or refuses the credentials, so start-up reports the problem.
    pub(crate) fn connect(settings: ClickHouseSettings) -> Result<Self, String> {
        check_endpoint(&settings.url)?;
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        std::thread::Builder::new()
            .name("audit-writer".to_string())
            .spawn(move || run_writer(settings, events_rx, ready_tx))
            .map_err(|error| format!("failed to start the audit writer: {error}"))?;
        ready_rx
            .recv()
            .map_err(|_| "audit writer exited before it was ready".to_string())??;
        Ok(Self { events: events_tx })
    }

    /// Records an event. Returns at once; the write happens on the writer thread.
    pub(crate) fn emit(&self, event: &'static str, task_id: &str, detail: &str) {
        let ts_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        let _ = self.events.send(Event {
            name: event,
            task_id: task_id.to_string(),
            detail: detail.to_string(),
            ts_millis,
        });
    }
}

fn run_writer(
    settings: ClickHouseSettings,
    mut events: mpsc::UnboundedReceiver<Event>,
    ready: std_mpsc::Sender<Result<(), String>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(format!("audit writer runtime: {error}")));
            return;
        }
    };
    runtime.block_on(async move {
        let client = match reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                let _ = ready.send(Err(format!("audit writer HTTP client: {error}")));
                return;
            }
        };
        if let Err(error) = execute(&client, &settings, SCHEMA).await {
            let _ = ready.send(Err(format!("ClickHouse audit table: {error}")));
            return;
        }
        let _ = ready.send(Ok(()));
        while let Some(event) = events.recv().await {
            let line = serde_json::json!({
                "ts_ms": event.ts_millis,
                "event": event.name,
                "task_id": event.task_id,
                "detail": event.detail,
            })
            .to_string();
            if let Err(error) = insert(&client, &settings, &line).await {
                // Best effort: report and keep going.
                eprintln!("audit event {} not written: {error}", event.name);
            }
        }
    });
}

/// Runs a statement that returns no rows.
async fn execute(
    client: &reqwest::Client,
    settings: &ClickHouseSettings,
    sql: &str,
) -> Result<(), String> {
    let response = client
        .post(&settings.url)
        .header("x-clickhouse-user", &settings.user)
        .header("x-clickhouse-key", &settings.password)
        .body(sql.to_string())
        .send()
        .await
        .map_err(|error| error.without_url().to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("ClickHouse answered {}", response.status()))
    }
}

async fn insert(
    client: &reqwest::Client,
    settings: &ClickHouseSettings,
    json_line: &str,
) -> Result<(), String> {
    let response = client
        .post(&settings.url)
        // Unknown fields are an error, not silently dropped: a schema mismatch must be visible.
        .query(&[
            ("query", format!("INSERT INTO {TABLE} FORMAT JSONEachRow")),
            ("input_format_skip_unknown_fields", "0".to_string()),
        ])
        .header("x-clickhouse-user", &settings.user)
        .header("x-clickhouse-key", &settings.password)
        .body(format!("{json_line}\n"))
        .send()
        .await
        .map_err(|error| error.without_url().to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("ClickHouse answered {}", response.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_travel_only_over_https_or_to_loopback() {
        assert!(check_endpoint("https://ch.internal:8443").is_ok());
        assert!(check_endpoint("http://127.0.0.1:8123").is_ok());
        assert!(check_endpoint("http://ch.internal:8123").is_err());
        assert!(check_endpoint("ftp://127.0.0.1").is_err());
        assert!(check_endpoint("not a url").is_err());
    }

    #[test]
    fn the_schema_names_the_columns_the_writer_sends() {
        for column in ["ts_ms", "event", "task_id", "detail"] {
            assert!(SCHEMA.contains(column), "{column}");
        }
        assert!(SCHEMA.contains(TABLE));
    }

    /// Writes one event to the `ClickHouse` named by `FRONTAL_TEST_CLICKHOUSE_URL` and reads it
    /// back. Without the variable the test returns early, like the other live-service tests.
    #[test]
    fn an_emitted_event_is_written_and_can_be_read_back() {
        let Ok(url) = std::env::var("FRONTAL_TEST_CLICKHOUSE_URL") else {
            return;
        };
        let settings = ClickHouseSettings {
            url: url.clone(),
            user: std::env::var("FRONTAL_TEST_CLICKHOUSE_USER")
                .unwrap_or_else(|_| "frontal".into()),
            password: std::env::var("FRONTAL_TEST_CLICKHOUSE_PASSWORD").unwrap_or_default(),
        };
        let sink = AuditSink::connect(settings.clone()).expect("connect audit sink");
        let mut bytes = [0_u8; 6];
        getrandom::getrandom(&mut bytes).expect("random");
        let task = format!("audit-test-{}", hex::encode(bytes));
        sink.emit("token.issued", &task, "ttl_secs=60");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let count = runtime.block_on(async {
            let client = reqwest::Client::new();
            for _ in 0..50 {
                let response = client
                    .post(&settings.url)
                    .header("x-clickhouse-user", &settings.user)
                    .header("x-clickhouse-key", &settings.password)
                    .body(format!(
                        "SELECT count() FROM {TABLE} WHERE task_id = '{task}' AND event = 'token.issued' AND ts_ms > 0 FORMAT TabSeparated"
                    ))
                    .send()
                    .await
                    .expect("query");
                let text = response.text().await.expect("body");
                let count: u64 = text.trim().parse().unwrap_or(0);
                if count > 0 {
                    return count;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            0
        });
        assert_eq!(count, 1, "the audit event never reached ClickHouse");
    }
}
