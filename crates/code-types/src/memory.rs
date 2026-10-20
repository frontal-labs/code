use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryScope {
    pub session_id: String,
    pub repo_id: Option<String>,
    pub branch_id: Option<String>,
}

impl MemoryScope {
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            repo_id: None,
            branch_id: None,
        }
    }

    #[must_use]
    pub fn with_repo_id(mut self, repo_id: impl Into<String>) -> Self {
        self.repo_id = Some(repo_id.into());
        self
    }

    #[must_use]
    pub fn with_branch_id(mut self, branch_id: impl Into<String>) -> Self {
        self.branch_id = Some(branch_id.into());
        self
    }
}

impl Default for MemoryScope {
    fn default() -> Self {
        Self::new("global")
    }
}
