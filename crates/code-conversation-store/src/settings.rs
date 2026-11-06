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
    /// The password would travel in cleartext to a non-loopback host.
    InsecureUrl(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDatabaseName(name) => write!(
                f,
                "invalid ClickHouse database name {name:?}: use only ASCII letters, digits and underscores"
            ),
            Self::InsecureUrl(url) => write!(
                f,
                "refusing to send the ClickHouse password over plaintext {url:?}: use https, or http only for a loopback host"
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
        validate_url_security(&self.url)?;
        self.admin_client()
            .query(&format!(
                "CREATE DATABASE IF NOT EXISTS `{}`",
                self.database
            ))
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

/// `https` is always accepted. Plain `http` is accepted only for a loopback host,
/// so a password is never sent in cleartext across a network.
fn validate_url_security(url: &str) -> Result<(), MigrationError> {
    let insecure = || MigrationError::Settings(SettingsError::InsecureUrl(url.to_owned()));
    if url.starts_with("https://") {
        return Ok(());
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(insecure());
    };
    let authority = rest.split('/').next().unwrap_or_default();
    // Drop any `user:pass@` prefix before taking the host.
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    };
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        Ok(())
    } else {
        Err(insecure())
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

    #[test]
    fn plaintext_http_is_only_allowed_for_loopback() {
        assert!(validate_url_security("https://ch.example.com:8443").is_ok());
        assert!(validate_url_security("http://localhost:8123").is_ok());
        assert!(validate_url_security("http://127.0.0.1:8123/path").is_ok());
        assert!(validate_url_security("http://[::1]:8123").is_ok());
        assert!(validate_url_security("http://user:pw@127.0.0.1:8123").is_ok());
        assert!(validate_url_security("http://ch.example.com:8123").is_err());
        assert!(validate_url_security("http://127.0.0.1.evil.example:8123").is_err());
        assert!(validate_url_security("ftp://localhost").is_err());
    }
}
