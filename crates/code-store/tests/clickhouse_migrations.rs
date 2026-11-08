//! Integration tests against a real `ClickHouse` server. They run when
//! `FRONTAL_TEST_CLICKHOUSE_URL` is set and are skipped otherwise, like the other
//! live-service suites:
//!
//! ```sh
//! docker compose -f infrastructure/compose/docker-compose.clickhouse.yml up -d
//! FRONTAL_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
//! FRONTAL_TEST_CLICKHOUSE_PASSWORD=dev \
//!   cargo test -p code-store
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use clickhouse::Row;
use code_store::{apply, ClickHouseSettings, MigrationError, MIGRATIONS};
use serde::Deserialize;

#[derive(Row, Deserialize)]
struct Count {
    n: u64,
}

#[derive(Row, Deserialize)]
struct TableName {
    name: String,
}

/// Each test gets its own database so runs cannot see each other's state.
fn settings_for_fresh_database() -> ClickHouseSettings {
    let url = std::env::var("FRONTAL_TEST_CLICKHOUSE_URL")
        .expect("FRONTAL_TEST_CLICKHOUSE_URL must name the server");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    ClickHouseSettings {
        url,
        user: std::env::var("FRONTAL_TEST_CLICKHOUSE_USER").unwrap_or_else(|_| "frontal".into()),
        password: std::env::var("FRONTAL_TEST_CLICKHOUSE_PASSWORD").unwrap_or_default(),
        database: format!("cs_test_{nanos}"),
    }
}

async fn drop_database(settings: &ClickHouseSettings) {
    settings
        .admin_client()
        .query(&format!("DROP DATABASE IF EXISTS {}", settings.database))
        .execute()
        .await
        .expect("drop test database");
}

#[tokio::test]
async fn applies_schema_once_and_is_idempotent() {
    if std::env::var("FRONTAL_TEST_CLICKHOUSE_URL").is_err() {
        return;
    }
    let settings = settings_for_fresh_database();
    let client = settings.connect().await.expect("connect");

    let first = apply(&client, MIGRATIONS).await.expect("first apply");
    assert_eq!(first.applied, vec![1]);

    let second = apply(&client, MIGRATIONS).await.expect("second apply");
    assert!(
        second.applied.is_empty(),
        "re-run must not re-apply: {second:?}"
    );

    let tables: Vec<TableName> = client
        .query("SELECT name FROM system.tables WHERE database = currentDatabase() ORDER BY name")
        .fetch_all()
        .await
        .expect("list tables");
    let names: Vec<String> = tables.into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        vec![
            "compactions",
            "embedding_models",
            "messages",
            "schema_migrations",
            "sessions",
            "tool_calls",
        ]
    );

    drop_database(&settings).await;
}

#[tokio::test]
async fn nullable_created_at_and_dedupe_key_behave_as_designed() {
    if std::env::var("FRONTAL_TEST_CLICKHOUSE_URL").is_err() {
        return;
    }
    let settings = settings_for_fresh_database();
    let client = settings.connect().await.expect("connect");
    apply(&client, MIGRATIONS).await.expect("apply");

    // Legacy rows have no created_at, so the partition key must accept NULL.
    // The same (user, session, seq, block) key arriving twice must collapse to one row.
    let insert = "INSERT INTO messages \
        (user_id, session_id, seq, block_idx, event_id, role, kind, text, model, provider, \
         input_tokens, output_tokens, legacy, created_at) \
        VALUES ('u1', 'session-1', 1, 0, generateUUIDv4(), 'user', 'message', 'hi', \
                'm', 'p', 0, 0, true, NULL)";
    client.query(insert).execute().await.expect("first insert");
    client
        .query(insert)
        .execute()
        .await
        .expect("duplicate insert");

    let count: Count = client
        .query("SELECT count() AS n FROM messages FINAL")
        .fetch_one()
        .await
        .expect("count");
    assert_eq!(
        count.n, 1,
        "duplicate (session_id, seq, block_idx) must dedupe"
    );

    drop_database(&settings).await;
}

#[tokio::test]
async fn refuses_to_run_when_an_applied_migration_was_edited() {
    if std::env::var("FRONTAL_TEST_CLICKHOUSE_URL").is_err() {
        return;
    }
    let settings = settings_for_fresh_database();
    let client = settings.connect().await.expect("connect");
    apply(&client, MIGRATIONS).await.expect("apply");

    client
        .query("INSERT INTO schema_migrations (version, name, checksum) VALUES (1, 'conversation_store', 'tampered')")
        .execute()
        .await
        .expect("tamper");

    let err = apply(&client, MIGRATIONS)
        .await
        .expect_err("tampered checksum must fail");
    assert!(
        matches!(err, MigrationError::ChecksumMismatch { version: 1, .. }),
        "unexpected error: {err}"
    );

    drop_database(&settings).await;
}

#[tokio::test]
async fn refuses_to_run_against_a_newer_schema() {
    if std::env::var("FRONTAL_TEST_CLICKHOUSE_URL").is_err() {
        return;
    }
    let settings = settings_for_fresh_database();
    let client = settings.connect().await.expect("connect");
    apply(&client, MIGRATIONS).await.expect("apply");

    client
        .query("INSERT INTO schema_migrations (version, name, checksum) VALUES (2, 'from_the_future', 'x')")
        .execute()
        .await
        .expect("insert newer version");

    let err = apply(&client, MIGRATIONS)
        .await
        .expect_err("newer schema must fail");
    assert!(
        matches!(err, MigrationError::UnknownApplied { version: 2 }),
        "unexpected error: {err}"
    );

    drop_database(&settings).await;
}

#[tokio::test]
async fn refuses_to_run_when_an_applied_migration_was_renamed() {
    if std::env::var("FRONTAL_TEST_CLICKHOUSE_URL").is_err() {
        return;
    }
    let settings = settings_for_fresh_database();
    let client = settings.connect().await.expect("connect");
    apply(&client, MIGRATIONS).await.expect("apply");

    let correct_checksum = code_store::checksum(MIGRATIONS[0].sql);
    client
        .query("INSERT INTO schema_migrations (version, name, checksum) VALUES (1, 'renamed', ?)")
        .bind(correct_checksum)
        .execute()
        .await
        .expect("rename");

    let err = apply(&client, MIGRATIONS)
        .await
        .expect_err("renamed migration must fail");
    assert!(
        matches!(err, MigrationError::NameMismatch { version: 1, .. }),
        "unexpected error: {err}"
    );

    drop_database(&settings).await;
}
