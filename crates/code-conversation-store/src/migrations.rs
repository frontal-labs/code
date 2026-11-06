use std::fmt;

use clickhouse::{Client, Row};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::settings::SettingsError;

/// One versioned schema change. The SQL is embedded at build time, so the
/// binary carries the exact schema it expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Every migration, in the order it must be applied. Append only: never edit
/// or reorder a migration that has been applied to a real database, because
/// its checksum is recorded and verified on every run.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "conversation_store",
    sql: include_str!("../migrations/clickhouse/0001_conversation_store.sql"),
}];

const CREATE_MIGRATIONS_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_migrations \
     (version UInt32, name String, checksum String, \
      applied_at DateTime64(3, 'UTC') DEFAULT now64(3)) \
     ENGINE = ReplacingMergeTree(applied_at) ORDER BY version";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Versions applied by this run, in order. Empty when the database is current.
    pub applied: Vec<u32>,
}

#[derive(Debug)]
pub enum MigrationError {
    Clickhouse(clickhouse::error::Error),
    Settings(SettingsError),
    InvalidRegistry(String),
    /// The database has a version this build does not know about, so it was
    /// migrated by a newer build. Refuse to run rather than guess.
    UnknownApplied {
        version: u32,
    },
    /// An applied migration's SQL no longer matches what was recorded.
    /// The migration at this version now has a different name, so it is not the same migration.
    NameMismatch {
        version: u32,
        recorded: String,
        expected: String,
    },
    ChecksumMismatch {
        version: u32,
        recorded: String,
        expected: String,
    },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clickhouse(err) => write!(f, "ClickHouse error: {err}"),
            Self::Settings(err) => write!(f, "{err}"),
            Self::InvalidRegistry(reason) => write!(f, "invalid migration registry: {reason}"),
            Self::UnknownApplied { version } => write!(
                f,
                "database has migration {version}, which this build does not know; refusing to run"
            ),
            Self::NameMismatch { version, recorded, expected } => write!(
                f,
                "migration {version} was recorded as {recorded:?} but the source is {expected:?}"
            ),
            Self::ChecksumMismatch { version, recorded, expected } => write!(
                f,
                "migration {version} was edited after it was applied (recorded {recorded}, source {expected})"
            ),
        }
    }
}

impl std::error::Error for MigrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Clickhouse(err) => Some(err),
            Self::Settings(err) => Some(err),
            _ => None,
        }
    }
}

impl From<clickhouse::error::Error> for MigrationError {
    fn from(err: clickhouse::error::Error) -> Self {
        Self::Clickhouse(err)
    }
}

#[derive(Row, Deserialize)]
struct AppliedRow {
    version: u32,
    name: String,
    checksum: String,
}

/// Hex-encoded SHA-256 of a migration's SQL.
#[must_use]
pub fn checksum(sql: &str) -> String {
    hex::encode(Sha256::digest(sql.as_bytes()))
}

/// Versions must be strictly increasing so the order is unambiguous.
pub fn validate_registry(migrations: &[Migration]) -> Result<(), MigrationError> {
    let mut previous = 0;
    for migration in migrations {
        if migration.version <= previous {
            return Err(MigrationError::InvalidRegistry(format!(
                "migration {} must come after version {previous}",
                migration.version
            )));
        }
        previous = migration.version;
    }
    Ok(())
}

/// Splits a migration file into statements. Line comments (`--`) are removed
/// first, so the file may document itself; statements are split on `;`. A `--`
/// inside a string literal is treated as a comment, so migrations must not contain one.
#[must_use]
pub fn split_statements(sql: &str) -> Vec<String> {
    let without_comments = sql
        .lines()
        .map(|line| line.find("--").map_or(line, |index| &line[..index]))
        .collect::<Vec<_>>()
        .join("\n");
    without_comments
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Applies every migration that is not yet recorded, in version order.
///
/// Safe to re-run. Each statement is `IF NOT EXISTS`, so a run that failed
/// partway is retried from the beginning of that migration. `ClickHouse` has no
/// DDL transactions, and two runs at once are not locked against each other;
/// run migrations from one place (a deploy step, or one replica at a time).
pub async fn apply(
    client: &Client,
    migrations: &[Migration],
) -> Result<MigrationReport, MigrationError> {
    validate_registry(migrations)?;
    client.query(CREATE_MIGRATIONS_TABLE).execute().await?;

    let recorded: Vec<AppliedRow> = client
        .query("SELECT version, name, checksum FROM schema_migrations FINAL")
        .fetch_all()
        .await?;

    for row in &recorded {
        let Some(migration) = migrations.iter().find(|m| m.version == row.version) else {
            return Err(MigrationError::UnknownApplied {
                version: row.version,
            });
        };
        if row.name != migration.name {
            return Err(MigrationError::NameMismatch {
                version: row.version,
                recorded: row.name.clone(),
                expected: migration.name.to_owned(),
            });
        }
        let expected = checksum(migration.sql);
        if row.checksum != expected {
            return Err(MigrationError::ChecksumMismatch {
                version: row.version,
                recorded: row.checksum.clone(),
                expected,
            });
        }
    }

    let mut applied = Vec::new();
    for migration in migrations {
        if recorded.iter().any(|row| row.version == migration.version) {
            continue;
        }
        for statement in split_statements(migration.sql) {
            client.query(&statement).execute().await?;
        }
        client
            .query("INSERT INTO schema_migrations (version, name, checksum) VALUES (?, ?, ?)")
            .bind(migration.version)
            .bind(migration.name)
            .bind(checksum(migration.sql))
            .execute()
            .await?;
        applied.push(migration.version);
    }

    Ok(MigrationReport { applied })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_strictly_increasing() {
        assert!(validate_registry(MIGRATIONS).is_ok());

        let duplicate = [
            Migration {
                version: 1,
                name: "a",
                sql: "SELECT 1",
            },
            Migration {
                version: 1,
                name: "b",
                sql: "SELECT 2",
            },
        ];
        assert!(matches!(
            validate_registry(&duplicate),
            Err(MigrationError::InvalidRegistry(_))
        ));
    }

    #[test]
    fn checksum_changes_with_sql() {
        assert_eq!(checksum("SELECT 1"), checksum("SELECT 1"));
        assert_ne!(checksum("SELECT 1"), checksum("SELECT 2"));
        assert_eq!(checksum("").len(), 64);
    }

    #[test]
    fn split_drops_comments_and_empty_statements() {
        let sql = "-- header; with semicolon\nCREATE TABLE a (x UInt8) ENGINE = Memory;\n\n-- trailing\n;\nSELECT 1 -- inline\n;";
        assert_eq!(
            split_statements(sql),
            vec![
                "CREATE TABLE a (x UInt8) ENGINE = Memory".to_owned(),
                "SELECT 1".to_owned()
            ]
        );
    }

    #[test]
    fn every_bundled_migration_is_idempotent_ddl() {
        for migration in MIGRATIONS {
            let statements = split_statements(migration.sql);
            assert!(
                !statements.is_empty(),
                "migration {} has no statements",
                migration.version
            );
            for statement in statements {
                let upper = statement.to_ascii_uppercase();
                let is_ddl =
                    upper.starts_with("CREATE TABLE") || upper.starts_with("CREATE DATABASE");
                assert!(
                    is_ddl,
                    "unexpected statement in migration {}: {statement}",
                    migration.version
                );
                assert!(
                    upper.contains("IF NOT EXISTS"),
                    "migration {} must be idempotent: {statement}",
                    migration.version
                );
            }
        }
    }
}
