//! Anonymous install identifier.
//!
//! A random UUID v4 stored in a single file under the config home. It is not
//! derived from the hostname, user name or any path, and it can be reset.

use std::io;
use std::path::Path;

use uuid::Uuid;

/// File name, relative to the config home, that stores the install id.
pub const ID_FILE_NAME: &str = "analytics_id";

/// Returns the stored install id, creating one if the file is missing or invalid.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot be written.
pub fn load_or_create(config_home: &Path) -> io::Result<String> {
    let path = config_home.join(ID_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if Uuid::parse_str(trimmed).is_ok() {
            return Ok(trimmed.to_string());
        }
    }
    std::fs::create_dir_all(config_home)?;
    let id = Uuid::new_v4().to_string();
    std::fs::write(&path, &id)?;
    Ok(id)
}

/// Deletes the stored install id. A missing file is not an error.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be removed.
pub fn reset(config_home: &Path) -> io::Result<()> {
    match std::fs::remove_file(config_home.join(ID_FILE_NAME)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "code-analytics-{name}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir should be created");
        dir
    }

    #[test]
    fn creates_then_reuses_the_same_id() {
        let dir = scratch_dir("reuse");
        let first = load_or_create(&dir).expect("id should be created");
        let second = load_or_create(&dir).expect("id should be reused");
        assert_eq!(first, second);
        assert!(Uuid::parse_str(&first).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn replaces_invalid_file_contents() {
        let dir = scratch_dir("invalid");
        std::fs::write(dir.join(ID_FILE_NAME), "not-a-uuid").expect("write should succeed");
        let id = load_or_create(&dir).expect("id should be regenerated");
        assert!(Uuid::parse_str(&id).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reset_removes_id_and_tolerates_missing_file() {
        let dir = scratch_dir("reset");
        let before = load_or_create(&dir).expect("id should be created");
        reset(&dir).expect("reset should succeed");
        assert!(!dir.join(ID_FILE_NAME).exists());
        reset(&dir).expect("reset of a missing file should succeed");
        let after = load_or_create(&dir).expect("id should be created again");
        assert_ne!(before, after);
        let _ = std::fs::remove_dir_all(dir);
    }
}
