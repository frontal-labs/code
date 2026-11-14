use std::fmt::{Display, Formatter};

/// Errors from setting up analytics. Recording an event never returns one of
/// these; callers only see them when constructing a backend.
#[derive(Debug)]
pub enum AnalyticsError {
    /// Reading or writing the anonymous install id failed.
    Io(std::io::Error),
    /// The backend client could not be created.
    Backend(String),
}

impl Display for AnalyticsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "analytics I/O error: {error}"),
            Self::Backend(message) => write!(f, "analytics backend error: {message}"),
        }
    }
}

impl std::error::Error for AnalyticsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Backend(_) => None,
        }
    }
}

impl From<std::io::Error> for AnalyticsError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
