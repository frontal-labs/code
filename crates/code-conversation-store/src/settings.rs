use std::fmt;

use clickhouse::Client;

use crate::migrations::MigrationError;

/// Where and how to reach `ClickHouse`. The password is never printed by `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct ClickHouseSettings {
    pub url: String,
    pub user: String,
    pub password: String,
    pub database: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// The database name must be a plain identifier because it is interpolated into DDL.
    InvalidDatabaseName(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDatabaseName(name) => write!(
                f,
                "invalid ClickHouse database name {name:?}: use only ASCII letters, digits and underscores"
            ),
        }
    }
}

impl std::error::Error for SettingsError {}

impl fmt::Debug for ClickHouseSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClickHouseSettings")
            .field("url", &self.url)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("database", &self.database)
            .finish()
    }
}

impl ClickHouseSettings {
    /// Client without a database selected, used for `CREATE DATABASE`.
    #[must_use]
    pub fn admin_client(&self) -> Client {
        Client::default()
            .with_url(&self.url)
            .with_user(&self.user)
            .with_password(&self.password)
    }

    /// Ensures the configured database exists and returns a client scoped to it.
    pub async fn connect(&self) -> Result<Client, MigrationError> {
        validate_database_name(&self.database)?;
        self.admin_client()
            .query(&format!("CREATE DATABASE IF NOT EXISTS {}", self.database))
            .execute()
            .await?;
        Ok(self.admin_client().with_database(&self.database))
    }
}

fn validate_database_name(name: &str) -> Result<(), MigrationError> {
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(MigrationError::Settings(
            SettingsError::InvalidDatabaseName(name.to_owned()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_password() {
        let settings = ClickHouseSettings {
            url: "http://localhost:8123".into(),
            user: "frontal".into(),
            password: "hunter2".into(),
            database: "frontal_code".into(),
        };
        let rendered = format!("{settings:?}");
        assert!(!rendered.contains("hunter2"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn database_names_must_be_plain_identifiers() {
        assert!(validate_database_name("frontal_code_1").is_ok());
        assert!(validate_database_name("").is_err());
        assert!(validate_database_name("x; DROP DATABASE y").is_err());
        assert!(validate_database_name("with-dash").is_err());
    }
}
