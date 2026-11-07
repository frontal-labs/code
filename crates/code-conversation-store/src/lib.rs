//! Storage for the conversation store: `ClickHouse` connection settings and the
//! versioned schema migrations. Ingest, redaction and embeddings arrive in
//! later phases (see `docs/design/conversation-store.md`).

mod migrations;
mod settings;

pub use migrations::{
    apply, checksum, split_statements, validate_registry, Migration, MigrationError,
    MigrationReport, MIGRATIONS,
};
pub use settings::{ClickHouseSettings, SettingsError};
