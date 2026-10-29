use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::json::{JsonError, JsonValue};
use crate::usage::TokenUsage;

/// Version written by this build. Version 2 adds message identity and ordering
/// (`message_id`, `seq`, `created_at_ms`, `parent_message_id`) and session metadata.
const SESSION_VERSION: u32 = 2;
/// Oldest version this build still loads. Version 1 files get identity assigned on load.
const MIN_SUPPORTED_SESSION_VERSION: u32 = 1;
const ROTATE_AFTER_BYTES: u64 = 256 * 1024;
const MAX_ROTATED_FILES: usize = 3;
/// Rotated copies the uploader has not confirmed are kept until this many bytes accumulate.
/// Only applies once an export cursor exists; before that, [`MAX_ROTATED_FILES`] applies.
const MAX_UNUPLOADED_ROTATED_BYTES: u64 = 16 * 1024 * 1024;
const ARCHIVE_DIR_NAME: &str = "archive";
const EXPORT_CURSOR_EXTENSION: &str = "export-cursor";
static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Speaker role associated with a persisted conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Structured message content stored inside a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        tool_use_id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
}

/// One conversation message with optional token-usage metadata.
///
/// Identity fields are `None` only for messages loaded from a version 1 file that
/// have not yet been written back. Anything pushed through a [`Session`] has all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub blocks: Vec<ContentBlock>,
    pub usage: Option<TokenUsage>,
    /// `UUIDv7` identifying this message across exports.
    pub message_id: Option<String>,
    /// Monotonic position within the session. Stable across compaction.
    pub seq: Option<u64>,
    pub created_at_ms: Option<u64>,
    pub parent_message_id: Option<String>,
}

impl ConversationMessage {
    #[must_use]
    pub fn new(role: MessageRole, blocks: Vec<ContentBlock>, usage: Option<TokenUsage>) -> Self {
        Self {
            role,
            blocks,
            usage,
            message_id: None,
            seq: None,
            created_at_ms: None,
            parent_message_id: None,
        }
    }
}

/// Descriptive metadata about where and how a session ran.
///
/// Every field is optional. Sessions written before this metadata existed load with all
/// fields unset.
impl SessionMetadata {
    /// Stable, non-reversible identifier for a repository remote. The remote URL itself is
    /// never stored, because it can contain credentials or private host names.
    #[must_use]
    pub fn repo_id_for_remote(remote_url: &str) -> String {
        let normalized = strip_remote_userinfo(remote_url.trim())
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_ascii_lowercase();
        let digest = Sha256::digest(normalized.as_bytes());
        format!("sha256:{}", hex_encode(&digest))
    }
}

/// Removes `user:token@` credentials from a remote URL. Credentials must never reach the hash
/// input, and the hash itself must not depend on them.
fn strip_remote_userinfo(remote: &str) -> String {
    let (scheme, rest) = match remote.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, remote),
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    match scheme {
        Some(scheme) => format!("{scheme}://{host}{path}"),
        None => format!("{host}{path}"),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMetadata {
    pub user_id: Option<String>,
    pub client: Option<String>,
    pub client_version: Option<String>,
    /// Hash of the repository remote, never the raw URL.
    pub repo_id: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
}

/// Metadata describing the latest compaction that summarized a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCompaction {
    pub count: u32,
    pub removed_message_count: usize,
    pub summary: String,
}

/// Provenance recorded when a session is forked from another session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFork {
    pub parent_session_id: String,
    pub branch_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionPersistence {
    path: PathBuf,
}

/// Persisted conversational state for the runtime and CLI session manager.
#[derive(Debug, Clone)]
pub struct Session {
    pub version: u32,
    pub session_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub messages: Vec<ConversationMessage>,
    pub compaction: Option<SessionCompaction>,
    pub fork: Option<SessionFork>,
    pub metadata: SessionMetadata,
    /// Sequence number the next pushed message will receive.
    pub next_seq: u64,
    persistence: Option<SessionPersistence>,
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.session_id == other.session_id
            && self.created_at_ms == other.created_at_ms
            && self.updated_at_ms == other.updated_at_ms
            && self.messages == other.messages
            && self.compaction == other.compaction
            && self.fork == other.fork
            && self.metadata == other.metadata
            && self.next_seq == other.next_seq
    }
}

impl Eq for Session {}

/// Errors raised while loading, parsing, or saving sessions.
#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Json(JsonError),
    Format(String),
}

impl Display for SessionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::Format(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<std::io::Error> for SessionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<JsonError> for SessionError {
    fn from(value: JsonError) -> Self {
        Self::Json(value)
    }
}

impl Session {
    #[must_use]
    pub fn new() -> Self {
        let now = current_time_millis();
        Self {
            version: SESSION_VERSION,
            session_id: generate_session_id(),
            created_at_ms: now,
            updated_at_ms: now,
            messages: Vec::new(),
            compaction: None,
            fork: None,
            metadata: SessionMetadata::default(),
            next_seq: 1,
            persistence: None,
        }
    }

    #[must_use]
    pub fn with_persistence_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.persistence = Some(SessionPersistence { path: path.into() });
        self
    }

    #[must_use]
    pub fn persistence_path(&self) -> Option<&Path> {
        self.persistence.as_ref().map(|value| value.path.as_path())
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let path = path.as_ref();
        let snapshot = self.render_jsonl_snapshot()?;
        rotate_session_file_if_needed(path)?;
        write_atomic(path, &snapshot)?;
        cleanup_rotated_logs(path)?;
        Ok(())
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        let session = match JsonValue::parse(&contents) {
            Ok(value)
                if value
                    .as_object()
                    .is_some_and(|object| object.contains_key("messages")) =>
            {
                Self::from_json(&value)?
            }
            Err(_) | Ok(_) => Self::from_jsonl(&contents)?,
        };
        Ok(session.with_persistence_path(path.to_path_buf()))
    }

    pub fn push_message(&mut self, mut message: ConversationMessage) -> Result<(), SessionError> {
        self.touch();
        let previous_next_seq = self.next_seq;
        let parent = self
            .messages
            .last()
            .and_then(|last| last.message_id.clone());
        assign_identity(&mut self.next_seq, &mut message, parent.as_deref());
        self.messages.push(message);
        let persist_result = {
            let message_ref = self.messages.last().ok_or_else(|| {
                SessionError::Format("message was just pushed but missing".to_string())
            })?;
            self.append_persisted_message(message_ref)
        };
        if let Err(error) = persist_result {
            self.messages.pop();
            self.next_seq = previous_next_seq;
            return Err(error);
        }
        Ok(())
    }

    /// Gives identity to messages that have none yet (new messages after compaction).
    /// Messages loaded from version 1 files keep their position-based `seq` and no ID,
    /// because inventing IDs on every load would break export idempotency.
    pub(crate) fn assign_missing_identity(&mut self) {
        let mut parent: Option<String> = None;
        for message in &mut self.messages {
            if message.seq.is_none() {
                assign_identity(&mut self.next_seq, message, parent.as_deref());
            }
            parent.clone_from(&message.message_id);
        }
    }

    /// Replaces this session's history with a compacted one.
    ///
    /// When `path` is given, the removed messages are appended to the session archive and the
    /// compacted snapshot is saved before this session changes. If either step fails, the
    /// original history stays in memory and on disk.
    pub fn apply_compaction(
        &mut self,
        compacted: Self,
        removed: &[ConversationMessage],
        path: Option<&Path>,
    ) -> Result<(), SessionError> {
        let Some(path) = path else {
            *self = compacted;
            return Ok(());
        };
        let compaction_count = compacted.compaction.as_ref().map_or(0, |value| value.count);
        append_archived_messages(path, compaction_count, removed)?;
        let mut next = compacted;
        next.persistence = Some(SessionPersistence {
            path: path.to_path_buf(),
        });
        next.save_to_path(path)?;
        *self = next;
        Ok(())
    }

    /// Records where and how the session runs. Only the fields that are `Some` in `metadata`
    /// replace the current values.
    pub fn set_metadata(&mut self, update: SessionMetadata) {
        let current = std::mem::take(&mut self.metadata);
        self.metadata = SessionMetadata {
            user_id: update.user_id.or(current.user_id),
            client: update.client.or(current.client),
            client_version: update.client_version.or(current.client_version),
            repo_id: update.repo_id.or(current.repo_id),
            branch: update.branch.or(current.branch),
            model: update.model.or(current.model),
            provider: update.provider.or(current.provider),
        };
    }

    pub fn push_user_text(&mut self, text: impl Into<String>) -> Result<(), SessionError> {
        self.push_message(ConversationMessage::user_text(text))
    }

    pub fn record_compaction(&mut self, summary: impl Into<String>, removed_message_count: usize) {
        self.touch();
        let count = self.compaction.as_ref().map_or(1, |value| value.count + 1);
        self.compaction = Some(SessionCompaction {
            count,
            removed_message_count,
            summary: summary.into(),
        });
    }

    /// Forks the session. The copied messages keep their IDs and sequence numbers, so
    /// `fork.parent_session_id` is the provenance link for shared history.
    #[must_use]
    pub fn fork(&self, branch_name: Option<String>) -> Self {
        let now = current_time_millis();
        Self {
            version: SESSION_VERSION,
            session_id: generate_session_id(),
            created_at_ms: now,
            updated_at_ms: now,
            messages: self.messages.clone(),
            compaction: self.compaction.clone(),
            fork: Some(SessionFork {
                parent_session_id: self.session_id.clone(),
                branch_name: normalize_optional_string(branch_name),
            }),
            metadata: self.metadata.clone(),
            next_seq: self.next_seq,
            persistence: None,
        }
    }

    pub fn to_json(&self) -> Result<JsonValue, SessionError> {
        let mut object = BTreeMap::new();
        object.insert(
            "version".to_string(),
            JsonValue::Number(i64::from(self.version)),
        );
        object.insert(
            "session_id".to_string(),
            JsonValue::String(self.session_id.clone()),
        );
        object.insert(
            "created_at_ms".to_string(),
            JsonValue::Number(i64_from_u64(self.created_at_ms, "created_at_ms")?),
        );
        object.insert(
            "updated_at_ms".to_string(),
            JsonValue::Number(i64_from_u64(self.updated_at_ms, "updated_at_ms")?),
        );
        object.insert(
            "messages".to_string(),
            JsonValue::Array(
                self.messages
                    .iter()
                    .map(ConversationMessage::to_json)
                    .collect(),
            ),
        );
        object.insert(
            "next_seq".to_string(),
            JsonValue::Number(i64_from_u64(self.next_seq, "next_seq")?),
        );
        insert_metadata(&mut object, &self.metadata);
        if let Some(compaction) = &self.compaction {
            object.insert("compaction".to_string(), compaction.to_json()?);
        }
        if let Some(fork) = &self.fork {
            object.insert("fork".to_string(), fork.to_json());
        }
        Ok(JsonValue::Object(object))
    }

    pub fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value
            .as_object()
            .ok_or_else(|| SessionError::Format("session must be an object".to_string()))?;
        let version = object
            .get("version")
            .and_then(JsonValue::as_i64)
            .ok_or_else(|| SessionError::Format("missing version".to_string()))?;
        let version = u32::try_from(version)
            .map_err(|_| SessionError::Format("version out of range".to_string()))?;
        let messages = object
            .get("messages")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| SessionError::Format("missing messages".to_string()))?
            .iter()
            .map(ConversationMessage::from_json)
            .collect::<Result<Vec<_>, _>>()?;
        let now = current_time_millis();
        let session_id = object
            .get("session_id")
            .and_then(JsonValue::as_str)
            .map_or_else(generate_session_id, ToOwned::to_owned);
        let created_at_ms = object
            .get("created_at_ms")
            .map(|value| required_u64_from_value(value, "created_at_ms"))
            .transpose()?
            .unwrap_or(now);
        let updated_at_ms = object
            .get("updated_at_ms")
            .map(|value| required_u64_from_value(value, "updated_at_ms"))
            .transpose()?
            .unwrap_or(created_at_ms);
        let compaction = object
            .get("compaction")
            .map(SessionCompaction::from_json)
            .transpose()?;
        let fork = object.get("fork").map(SessionFork::from_json).transpose()?;
        let recorded_next_seq = object
            .get("next_seq")
            .map(|value| required_u64_from_value(value, "next_seq"))
            .transpose()?;
        Ok(finish_loaded(
            Self {
                version,
                session_id,
                created_at_ms,
                updated_at_ms,
                messages,
                compaction,
                fork,
                metadata: metadata_from_object(object),
                next_seq: 1,
                persistence: None,
            },
            recorded_next_seq,
        ))
    }

    #[allow(clippy::too_many_lines)]
    fn from_jsonl(contents: &str) -> Result<Self, SessionError> {
        let mut version = SESSION_VERSION;
        let mut session_id = None;
        let mut created_at_ms = None;
        let mut updated_at_ms = None;
        let mut messages = Vec::new();
        let mut compaction = None;
        let mut fork = None;
        let mut metadata = SessionMetadata::default();
        let mut recorded_next_seq = None;

        // A crash in the middle of an append leaves an unterminated last line. That write
        // was never acknowledged, so it is ignored. A bad line anywhere else is corruption.
        let last_content_line = contents
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .last()
            .map(|(index, _)| index);
        let torn_tail_possible = !contents.ends_with('\n');

        for (line_number, raw_line) in contents.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            let value = match JsonValue::parse(line) {
                Ok(value) => value,
                Err(_) if torn_tail_possible && Some(line_number) == last_content_line => {
                    break;
                }
                Err(error) => {
                    return Err(SessionError::Format(format!(
                        "invalid JSONL record at line {}: {}",
                        line_number + 1,
                        error
                    )));
                }
            };
            let object = value.as_object().ok_or_else(|| {
                SessionError::Format(format!(
                    "JSONL record at line {} must be an object",
                    line_number + 1
                ))
            })?;
            match object
                .get("type")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    SessionError::Format(format!(
                        "JSONL record at line {} missing type",
                        line_number + 1
                    ))
                })? {
                "session_meta" => {
                    version = required_u32(object, "version")?;
                    session_id = Some(required_string(object, "session_id")?);
                    created_at_ms = Some(required_u64(object, "created_at_ms")?);
                    updated_at_ms = Some(required_u64(object, "updated_at_ms")?);
                    fork = object.get("fork").map(SessionFork::from_json).transpose()?;
                    metadata = metadata_from_object(object);
                    recorded_next_seq = object
                        .get("next_seq")
                        .map(|value| required_u64_from_value(value, "next_seq"))
                        .transpose()?;
                }
                "message" => {
                    let message_value = object.get("message").ok_or_else(|| {
                        SessionError::Format(format!(
                            "JSONL record at line {} missing message",
                            line_number + 1
                        ))
                    })?;
                    messages.push(ConversationMessage::from_json(message_value)?);
                }
                "compaction" => {
                    compaction = Some(SessionCompaction::from_json(&JsonValue::Object(
                        object.clone(),
                    ))?);
                }
                other => {
                    return Err(SessionError::Format(format!(
                        "unsupported JSONL record type at line {}: {other}",
                        line_number + 1
                    )))
                }
            }
        }

        if !(MIN_SUPPORTED_SESSION_VERSION..=SESSION_VERSION).contains(&version) {
            return Err(SessionError::Format(format!(
                "unsupported session version {version}"
            )));
        }
        let now = current_time_millis();
        Ok(finish_loaded(
            Self {
                version,
                session_id: session_id.unwrap_or_else(generate_session_id),
                created_at_ms: created_at_ms.unwrap_or(now),
                updated_at_ms: updated_at_ms.unwrap_or(created_at_ms.unwrap_or(now)),
                messages,
                compaction,
                fork,
                metadata,
                next_seq: 1,
                persistence: None,
            },
            recorded_next_seq,
        ))
    }

    fn render_jsonl_snapshot(&self) -> Result<String, SessionError> {
        let mut lines = vec![self.meta_record()?.render()];
        if let Some(compaction) = &self.compaction {
            lines.push(compaction.to_jsonl_record()?.render());
        }
        lines.extend(
            self.messages
                .iter()
                .map(|message| message_record(message).render()),
        );
        let mut rendered = lines.join("\n");
        rendered.push('\n');
        Ok(rendered)
    }

    fn append_persisted_message(&self, message: &ConversationMessage) -> Result<(), SessionError> {
        let Some(path) = self.persistence_path() else {
            return Ok(());
        };

        // The first append to an older file writes a full snapshot, so the header and the
        // identity fields are upgraded in one step.
        if self.version < SESSION_VERSION || !repair_torn_tail(path)? {
            self.save_to_path(path)?;
            return Ok(());
        }

        let mut file = OpenOptions::new().append(true).open(path)?;
        writeln!(file, "{}", message_record(message).render())?;
        file.sync_data()?;
        Ok(())
    }

    fn meta_record(&self) -> Result<JsonValue, SessionError> {
        let mut object = BTreeMap::new();
        object.insert(
            "type".to_string(),
            JsonValue::String("session_meta".to_string()),
        );
        object.insert(
            "version".to_string(),
            JsonValue::Number(i64::from(SESSION_VERSION)),
        );
        object.insert(
            "session_id".to_string(),
            JsonValue::String(self.session_id.clone()),
        );
        object.insert(
            "created_at_ms".to_string(),
            JsonValue::Number(i64_from_u64(self.created_at_ms, "created_at_ms")?),
        );
        object.insert(
            "updated_at_ms".to_string(),
            JsonValue::Number(i64_from_u64(self.updated_at_ms, "updated_at_ms")?),
        );
        if let Some(fork) = &self.fork {
            object.insert("fork".to_string(), fork.to_json());
        }
        object.insert(
            "next_seq".to_string(),
            JsonValue::Number(i64_from_u64(self.next_seq, "next_seq")?),
        );
        insert_metadata(&mut object, &self.metadata);
        Ok(JsonValue::Object(object))
    }

    fn touch(&mut self) {
        self.updated_at_ms = current_time_millis();
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationMessage {
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::new(
            MessageRole::User,
            vec![ContentBlock::Text { text: text.into() }],
            None,
        )
    }

    #[must_use]
    pub fn assistant(blocks: Vec<ContentBlock>) -> Self {
        Self::new(MessageRole::Assistant, blocks, None)
    }

    #[must_use]
    pub fn assistant_with_usage(blocks: Vec<ContentBlock>, usage: Option<TokenUsage>) -> Self {
        Self::new(MessageRole::Assistant, blocks, usage)
    }

    #[must_use]
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        tool_name: impl Into<String>,
        output: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self::new(
            MessageRole::Tool,
            vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                tool_name: tool_name.into(),
                output: output.into(),
                is_error,
            }],
            None,
        )
    }

    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = BTreeMap::new();
        object.insert(
            "role".to_string(),
            JsonValue::String(
                match self.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                }
                .to_string(),
            ),
        );
        object.insert(
            "blocks".to_string(),
            JsonValue::Array(self.blocks.iter().map(ContentBlock::to_json).collect()),
        );
        if let Some(usage) = self.usage {
            object.insert("usage".to_string(), usage_to_json(usage));
        }
        if let Some(message_id) = &self.message_id {
            object.insert(
                "message_id".to_string(),
                JsonValue::String(message_id.clone()),
            );
        }
        if let Some(seq) = self.seq {
            object.insert(
                "seq".to_string(),
                JsonValue::Number(i64::try_from(seq).unwrap_or(i64::MAX)),
            );
        }
        if let Some(created_at_ms) = self.created_at_ms {
            object.insert(
                "created_at_ms".to_string(),
                JsonValue::Number(i64::try_from(created_at_ms).unwrap_or(i64::MAX)),
            );
        }
        if let Some(parent) = &self.parent_message_id {
            object.insert(
                "parent_message_id".to_string(),
                JsonValue::String(parent.clone()),
            );
        }
        JsonValue::Object(object)
    }

    fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value
            .as_object()
            .ok_or_else(|| SessionError::Format("message must be an object".to_string()))?;
        let role = match object
            .get("role")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| SessionError::Format("missing role".to_string()))?
        {
            "system" => MessageRole::System,
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "tool" => MessageRole::Tool,
            other => {
                return Err(SessionError::Format(format!(
                    "unsupported message role: {other}"
                )))
            }
        };
        let blocks = object
            .get("blocks")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| SessionError::Format("missing blocks".to_string()))?
            .iter()
            .map(ContentBlock::from_json)
            .collect::<Result<Vec<_>, _>>()?;
        let usage = object.get("usage").map(usage_from_json).transpose()?;
        let optional_string = |key: &str| {
            object
                .get(key)
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned)
        };
        Ok(Self {
            role,
            blocks,
            usage,
            message_id: optional_string("message_id"),
            seq: object
                .get("seq")
                .map(|value| required_u64_from_value(value, "seq"))
                .transpose()?,
            created_at_ms: object
                .get("created_at_ms")
                .map(|value| required_u64_from_value(value, "created_at_ms"))
                .transpose()?,
            parent_message_id: optional_string("parent_message_id"),
        })
    }
}

impl ContentBlock {
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = BTreeMap::new();
        match self {
            Self::Text { text } => {
                object.insert("type".to_string(), JsonValue::String("text".to_string()));
                object.insert("text".to_string(), JsonValue::String(text.clone()));
            }
            Self::ToolUse { id, name, input } => {
                object.insert(
                    "type".to_string(),
                    JsonValue::String("tool_use".to_string()),
                );
                object.insert("id".to_string(), JsonValue::String(id.clone()));
                object.insert("name".to_string(), JsonValue::String(name.clone()));
                object.insert("input".to_string(), JsonValue::String(input.clone()));
            }
            Self::ToolResult {
                tool_use_id,
                tool_name,
                output,
                is_error,
            } => {
                object.insert(
                    "type".to_string(),
                    JsonValue::String("tool_result".to_string()),
                );
                object.insert(
                    "tool_use_id".to_string(),
                    JsonValue::String(tool_use_id.clone()),
                );
                object.insert(
                    "tool_name".to_string(),
                    JsonValue::String(tool_name.clone()),
                );
                object.insert("output".to_string(), JsonValue::String(output.clone()));
                object.insert("is_error".to_string(), JsonValue::Bool(*is_error));
            }
        }
        JsonValue::Object(object)
    }

    fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value
            .as_object()
            .ok_or_else(|| SessionError::Format("block must be an object".to_string()))?;
        match object
            .get("type")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| SessionError::Format("missing block type".to_string()))?
        {
            "text" => Ok(Self::Text {
                text: required_string(object, "text")?,
            }),
            "tool_use" => Ok(Self::ToolUse {
                id: required_string(object, "id")?,
                name: required_string(object, "name")?,
                input: required_string(object, "input")?,
            }),
            "tool_result" => Ok(Self::ToolResult {
                tool_use_id: required_string(object, "tool_use_id")?,
                tool_name: required_string(object, "tool_name")?,
                output: required_string(object, "output")?,
                is_error: object
                    .get("is_error")
                    .and_then(JsonValue::as_bool)
                    .ok_or_else(|| SessionError::Format("missing is_error".to_string()))?,
            }),
            other => Err(SessionError::Format(format!(
                "unsupported block type: {other}"
            ))),
        }
    }
}

impl SessionCompaction {
    pub fn to_json(&self) -> Result<JsonValue, SessionError> {
        let mut object = BTreeMap::new();
        object.insert(
            "count".to_string(),
            JsonValue::Number(i64::from(self.count)),
        );
        object.insert(
            "removed_message_count".to_string(),
            JsonValue::Number(i64_from_usize(
                self.removed_message_count,
                "removed_message_count",
            )?),
        );
        object.insert(
            "summary".to_string(),
            JsonValue::String(self.summary.clone()),
        );
        Ok(JsonValue::Object(object))
    }

    pub fn to_jsonl_record(&self) -> Result<JsonValue, SessionError> {
        let mut object = BTreeMap::new();
        object.insert(
            "type".to_string(),
            JsonValue::String("compaction".to_string()),
        );
        object.insert(
            "count".to_string(),
            JsonValue::Number(i64::from(self.count)),
        );
        object.insert(
            "removed_message_count".to_string(),
            JsonValue::Number(i64_from_usize(
                self.removed_message_count,
                "removed_message_count",
            )?),
        );
        object.insert(
            "summary".to_string(),
            JsonValue::String(self.summary.clone()),
        );
        Ok(JsonValue::Object(object))
    }

    fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value
            .as_object()
            .ok_or_else(|| SessionError::Format("compaction must be an object".to_string()))?;
        Ok(Self {
            count: required_u32(object, "count")?,
            removed_message_count: required_usize(object, "removed_message_count")?,
            summary: required_string(object, "summary")?,
        })
    }
}

impl SessionFork {
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = BTreeMap::new();
        object.insert(
            "parent_session_id".to_string(),
            JsonValue::String(self.parent_session_id.clone()),
        );
        if let Some(branch_name) = &self.branch_name {
            object.insert(
                "branch_name".to_string(),
                JsonValue::String(branch_name.clone()),
            );
        }
        JsonValue::Object(object)
    }

    fn from_json(value: &JsonValue) -> Result<Self, SessionError> {
        let object = value
            .as_object()
            .ok_or_else(|| SessionError::Format("fork metadata must be an object".to_string()))?;
        Ok(Self {
            parent_session_id: required_string(object, "parent_session_id")?,
            branch_name: object
                .get("branch_name")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
        })
    }
}

fn message_record(message: &ConversationMessage) -> JsonValue {
    let mut object = BTreeMap::new();
    object.insert("type".to_string(), JsonValue::String("message".to_string()));
    object.insert("message".to_string(), message.to_json());
    JsonValue::Object(object)
}

fn usage_to_json(usage: TokenUsage) -> JsonValue {
    let mut object = BTreeMap::new();
    object.insert(
        "input_tokens".to_string(),
        JsonValue::Number(i64::from(usage.input_tokens)),
    );
    object.insert(
        "output_tokens".to_string(),
        JsonValue::Number(i64::from(usage.output_tokens)),
    );
    object.insert(
        "cache_creation_input_tokens".to_string(),
        JsonValue::Number(i64::from(usage.cache_creation_input_tokens)),
    );
    object.insert(
        "cache_read_input_tokens".to_string(),
        JsonValue::Number(i64::from(usage.cache_read_input_tokens)),
    );
    JsonValue::Object(object)
}

fn usage_from_json(value: &JsonValue) -> Result<TokenUsage, SessionError> {
    let object = value
        .as_object()
        .ok_or_else(|| SessionError::Format("usage must be an object".to_string()))?;
    Ok(TokenUsage {
        input_tokens: required_u32(object, "input_tokens")?,
        output_tokens: required_u32(object, "output_tokens")?,
        cache_creation_input_tokens: required_u32(object, "cache_creation_input_tokens")?,
        cache_read_input_tokens: required_u32(object, "cache_read_input_tokens")?,
    })
}

fn required_string(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<String, SessionError> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))
}

fn required_u32(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<u32, SessionError> {
    let value = object
        .get(key)
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))?;
    u32::try_from(value).map_err(|_| SessionError::Format(format!("{key} out of range")))
}

fn required_u64(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<u64, SessionError> {
    let value = object
        .get(key)
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))?;
    required_u64_from_value(value, key)
}

fn required_u64_from_value(value: &JsonValue, key: &str) -> Result<u64, SessionError> {
    let value = value
        .as_i64()
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))?;
    u64::try_from(value).map_err(|_| SessionError::Format(format!("{key} out of range")))
}

fn required_usize(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<usize, SessionError> {
    let value = object
        .get(key)
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| SessionError::Format(format!("missing {key}")))?;
    usize::try_from(value).map_err(|_| SessionError::Format(format!("{key} out of range")))
}

fn i64_from_u64(value: u64, key: &str) -> Result<i64, SessionError> {
    i64::try_from(value)
        .map_err(|_| SessionError::Format(format!("{key} out of range for JSON number")))
}

fn i64_from_usize(value: usize, key: &str) -> Result<i64, SessionError> {
    i64::try_from(value)
        .map_err(|_| SessionError::Format(format!("{key} out of range for JSON number")))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn generate_session_id() -> String {
    let millis = current_time_millis();
    let counter = SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("session-{millis}-{counter}")
}

/// Gives a message its sequence number, `UUIDv7`, timestamp and parent link.
/// Existing values are kept, so a message keeps its identity when it is copied.
fn assign_identity(next_seq: &mut u64, message: &mut ConversationMessage, parent: Option<&str>) {
    let seq = if let Some(seq) = message.seq {
        seq
    } else {
        let seq = *next_seq;
        message.seq = Some(seq);
        seq
    };
    *next_seq = (*next_seq).max(seq.saturating_add(1));
    message
        .message_id
        .get_or_insert_with(|| uuid::Uuid::now_v7().to_string());
    message
        .created_at_ms
        .get_or_insert_with(current_time_millis);
    if message.parent_message_id.is_none() {
        message.parent_message_id = parent.map(ToOwned::to_owned);
    }
}

/// Finishes loading a session from any on-disk format. Messages without a `seq` (version 1
/// files) are numbered by position, starting at 1.
fn finish_loaded(mut session: Session, recorded_next_seq: Option<u64>) -> Session {
    for (index, message) in session.messages.iter_mut().enumerate() {
        if message.seq.is_none() {
            message.seq = Some(index as u64 + 1);
        }
    }
    let highest_seq = session
        .messages
        .iter()
        .filter_map(|message| message.seq)
        .max()
        .unwrap_or(0);
    session.next_seq = recorded_next_seq
        .unwrap_or(0)
        .max(highest_seq.saturating_add(1));
    session
}

fn insert_metadata(object: &mut BTreeMap<String, JsonValue>, metadata: &SessionMetadata) {
    let fields = [
        ("user_id", &metadata.user_id),
        ("client", &metadata.client),
        ("client_version", &metadata.client_version),
        ("repo_id", &metadata.repo_id),
        ("branch", &metadata.branch),
        ("model", &metadata.model),
        ("provider", &metadata.provider),
    ];
    for (key, value) in fields {
        if let Some(value) = value {
            object.insert(key.to_string(), JsonValue::String(value.clone()));
        }
    }
}

fn metadata_from_object(object: &BTreeMap<String, JsonValue>) -> SessionMetadata {
    let read = |key: &str| {
        object
            .get(key)
            .and_then(JsonValue::as_str)
            .map(ToOwned::to_owned)
    };
    SessionMetadata {
        user_id: read("user_id"),
        client: read("client"),
        client_version: read("client_version"),
        repo_id: read("repo_id"),
        branch: read("branch"),
        model: read("model"),
        provider: read("provider"),
    }
}

/// Ensures the file ends at a line boundary, dropping an unterminated fragment left by a
/// crash. Returns `false` when no complete line remains, so the caller must write a snapshot.
fn repair_torn_tail(path: &Path) -> Result<bool, SessionError> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last_byte = [0_u8; 1];
    file.read_exact(&mut last_byte)?;
    if last_byte[0] == b'\n' {
        return Ok(true);
    }
    let mut contents = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut contents)?;
    let keep = contents
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    file.set_len(keep as u64)?;
    file.sync_all()?;
    Ok(keep > 0)
}

/// Appends removed messages to `<dir>/archive/<session>.jsonl` and makes them durable.
/// The archive is what keeps pre-compaction messages after the live file drops them.
fn append_archived_messages(
    session_path: &Path,
    compaction_count: u32,
    removed: &[ConversationMessage],
) -> Result<(), SessionError> {
    if removed.is_empty() {
        return Ok(());
    }
    let parent = session_path.parent().unwrap_or_else(|| Path::new("."));
    let archive_dir = parent.join(ARCHIVE_DIR_NAME);
    create_private_dir(&archive_dir)?;
    let archive_path = archive_dir.join(format!("{}.jsonl", file_stem_or_default(session_path)));
    repair_torn_tail(&archive_path)?;
    let mut lines = String::new();
    for message in removed {
        let mut object = BTreeMap::new();
        object.insert(
            "type".to_string(),
            JsonValue::String("archived_message".to_string()),
        );
        object.insert(
            "compaction".to_string(),
            JsonValue::Number(i64::from(compaction_count)),
        );
        object.insert("message".to_string(), message.to_json());
        lines.push_str(&JsonValue::Object(object).render());
        lines.push('\n');
    }
    let mut file = if archive_path.exists() {
        OpenOptions::new().append(true).open(&archive_path)?
    } else {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&archive_path)?
    };
    file.write_all(lines.as_bytes())?;
    file.sync_data()?;
    Ok(())
}

fn file_stem_or_default(path: &Path) -> &str {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("session")
}

/// Reads the highest sequence number the uploader has confirmed for this session.
/// Returns `None` when no cursor exists yet.
#[must_use]
pub fn read_export_cursor(session_path: &Path) -> Option<u64> {
    let contents = fs::read_to_string(export_cursor_path(session_path)).ok()?;
    let value = JsonValue::parse(&contents).ok()?;
    value
        .as_object()?
        .get("uploaded_through_seq")?
        .as_i64()
        .and_then(|seq| u64::try_from(seq).ok())
}

/// Records that everything up to `seq` has been uploaded. Rotated copies at or below this
/// sequence number may be deleted once they are no longer needed.
pub fn write_export_cursor(session_path: &Path, seq: u64) -> Result<(), SessionError> {
    let mut object = BTreeMap::new();
    object.insert(
        "uploaded_through_seq".to_string(),
        JsonValue::Number(i64_from_u64(seq, "uploaded_through_seq")?),
    );
    write_atomic(
        &export_cursor_path(session_path),
        &format!("{}\n", JsonValue::Object(object).render()),
    )
}

fn export_cursor_path(session_path: &Path) -> PathBuf {
    session_path.with_file_name(format!(
        "{}.{EXPORT_CURSOR_EXTENSION}",
        file_stem_or_default(session_path)
    ))
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), SessionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = temporary_path_for(path);
    let result = write_temp_then_rename(&temp_path, path, contents);
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Writes the full file, syncs it, renames it over `path`, then syncs the directory. A crash
/// leaves either the old file or the new one, never a partial file.
fn write_temp_then_rename(
    temp_path: &Path,
    path: &Path,
    contents: &str,
) -> Result<(), SessionError> {
    let mut file = create_private(temp_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp_path, path)?;
    sync_parent_dir(path)
}

/// Creates a file readable only by its owner. Session files hold conversation text, so the
/// default umask-based mode is too permissive.
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

fn sync_parent_dir(path: &Path) -> Result<(), SessionError> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn temporary_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    path.with_file_name(format!(
        "{file_name}.tmp-{}-{}",
        current_time_millis(),
        SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Copies the live file to a rotated backup once it is large. The live file is never moved
/// away, so a crash at any point leaves the session readable.
fn rotate_session_file_if_needed(path: &Path) -> Result<(), SessionError> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() < ROTATE_AFTER_BYTES {
        return Ok(());
    }
    // Every save rewrites the whole file, so rotating on each save would copy it every turn.
    // Rotate again only after the file has grown by another threshold past the newest copy.
    if let Some(newest) = list_rotated_logs(path)?.last() {
        if metadata.len() < newest.bytes.saturating_add(ROTATE_AFTER_BYTES) {
            return Ok(());
        }
    }
    let contents = fs::read_to_string(path)?;
    let rotated_path = rotated_log_path(path, last_message_seq(&contents));
    write_atomic(&rotated_path, &contents)
}

/// Highest `seq` among the message records in a session file, or 0 when there are none.
fn last_message_seq(contents: &str) -> u64 {
    contents
        .lines()
        .rev()
        .filter_map(|line| JsonValue::parse(line.trim()).ok())
        .find_map(|value| {
            let object = value.as_object()?;
            if object.get("type")?.as_str()? != "message" {
                return None;
            }
            object
                .get("message")?
                .as_object()?
                .get("seq")?
                .as_i64()
                .and_then(|seq| u64::try_from(seq).ok())
        })
        .unwrap_or(0)
}

fn rotated_log_path(path: &Path, through_seq: u64) -> PathBuf {
    path.with_file_name(format!(
        "{}.rot-{}-s{through_seq}.jsonl",
        file_stem_or_default(path),
        current_time_millis()
    ))
}

struct RotatedLog {
    path: PathBuf,
    through_seq: u64,
    bytes: u64,
}

fn list_rotated_logs(path: &Path) -> Result<Vec<RotatedLog>, SessionError> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    let prefix = format!("{}.rot-", file_stem_or_default(path));
    let mut logs = Vec::new();
    for entry in fs::read_dir(parent)?.filter_map(Result::ok) {
        let entry_path = entry.path();
        let Some(name) = entry_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix)
            || !Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        {
            continue;
        }
        // Rotated files from before sequence numbers were recorded count as uploaded.
        let through_seq = name
            .strip_suffix(".jsonl")
            .and_then(|stem| stem.rsplit_once("-s"))
            .and_then(|(_, seq)| seq.parse::<u64>().ok())
            .unwrap_or(0);
        let bytes = entry.metadata().map_or(0, |metadata| metadata.len());
        logs.push(RotatedLog {
            path: entry_path,
            through_seq,
            bytes,
        });
    }
    logs.sort_by_key(|log| log.through_seq);
    Ok(logs)
}

/// Deletes rotated copies that are no longer needed.
///
/// A copy is needed until the export cursor passes its highest sequence number. Uploaded
/// copies beyond the newest [`MAX_ROTATED_FILES`] are deleted. Un-uploaded copies are deleted
/// oldest first, but only once they exceed [`MAX_UNUPLOADED_ROTATED_BYTES`].
fn cleanup_rotated_logs(path: &Path) -> Result<(), SessionError> {
    // Without an uploader there is nothing to wait for, so keep only the newest copies.
    // Rotated copies hold sensitive conversation text, so they are not kept longer than needed.
    let Some(uploaded_through) = read_export_cursor(path) else {
        let logs = list_rotated_logs(path)?;
        let excess = logs.len().saturating_sub(MAX_ROTATED_FILES);
        for stale in logs.iter().take(excess) {
            fs::remove_file(&stale.path)?;
        }
        return Ok(());
    };
    let (uploaded, mut pending): (Vec<_>, Vec<_>) = list_rotated_logs(path)?
        .into_iter()
        .partition(|log| log.through_seq <= uploaded_through);

    let uploaded_excess = uploaded.len().saturating_sub(MAX_ROTATED_FILES);
    for stale in uploaded.iter().take(uploaded_excess) {
        fs::remove_file(&stale.path)?;
    }

    let mut pending_bytes: u64 = pending.iter().map(|log| log.bytes).sum();
    while pending_bytes > MAX_UNUPLOADED_ROTATED_BYTES && !pending.is_empty() {
        let oldest = pending.remove(0);
        fs::remove_file(&oldest.path)?;
        pending_bytes = pending_bytes.saturating_sub(oldest.bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cleanup_rotated_logs, rotate_session_file_if_needed, ContentBlock, ConversationMessage,
        MessageRole, Session, SessionFork,
    };
    use crate::json::JsonValue;
    use crate::usage::TokenUsage;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn persists_and_restores_session_jsonl() {
        let mut session = Session::new();
        session
            .push_user_text("hello")
            .expect("user message should append");
        session
            .push_message(ConversationMessage::assistant_with_usage(
                vec![
                    ContentBlock::Text {
                        text: "thinking".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "bash".to_string(),
                        input: "echo hi".to_string(),
                    },
                ],
                Some(TokenUsage {
                    input_tokens: 10,
                    output_tokens: 4,
                    cache_creation_input_tokens: 1,
                    cache_read_input_tokens: 2,
                }),
            ))
            .expect("assistant message should append");
        session
            .push_message(ConversationMessage::tool_result(
                "tool-1", "bash", "hi", false,
            ))
            .expect("tool result should append");

        let path = temp_session_path("jsonl");
        session.save_to_path(&path).expect("session should save");
        let restored = Session::load_from_path(&path).expect("session should load");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored, session);
        assert_eq!(restored.messages[2].role, MessageRole::Tool);
        assert_eq!(
            restored.messages[1].usage.expect("usage").total_tokens(),
            17
        );
        assert_eq!(restored.session_id, session.session_id);
    }

    #[test]
    fn loads_legacy_session_json_object() {
        let path = temp_session_path("legacy");
        let legacy = JsonValue::Object(
            [
                ("version".to_string(), JsonValue::Number(1)),
                (
                    "messages".to_string(),
                    JsonValue::Array(vec![ConversationMessage::user_text("legacy").to_json()]),
                ),
            ]
            .into_iter()
            .collect(),
        );
        fs::write(&path, legacy.render()).expect("legacy file should write");

        let restored = Session::load_from_path(&path).expect("legacy session should load");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored.messages.len(), 1);
        assert_eq!(restored.messages[0].role, MessageRole::User);
        assert_eq!(
            restored.messages[0].blocks,
            ConversationMessage::user_text("legacy").blocks
        );
        assert_ne!(restored.session_id.len(), 0);
    }

    #[test]
    fn appends_messages_to_persisted_jsonl_session() {
        let path = temp_session_path("append");
        let mut session = Session::new().with_persistence_path(path.clone());
        session
            .save_to_path(&path)
            .expect("initial save should succeed");
        session
            .push_user_text("hi")
            .expect("user append should succeed");
        session
            .push_message(ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "hello".to_string(),
            }]))
            .expect("assistant append should succeed");

        let restored = Session::load_from_path(&path).expect("session should replay from jsonl");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored.messages.len(), 2);
        assert_eq!(
            restored.messages[0].blocks,
            ConversationMessage::user_text("hi").blocks
        );
    }

    #[test]
    fn persists_compaction_metadata() {
        let path = temp_session_path("compaction");
        let mut session = Session::new();
        session
            .push_user_text("before")
            .expect("message should append");
        session.record_compaction("summarized earlier work", 4);
        session.save_to_path(&path).expect("session should save");

        let restored = Session::load_from_path(&path).expect("session should load");
        fs::remove_file(&path).expect("temp file should be removable");

        let compaction = restored.compaction.expect("compaction metadata");
        assert_eq!(compaction.count, 1);
        assert_eq!(compaction.removed_message_count, 4);
        assert!(compaction.summary.contains("summarized"));
    }

    #[test]
    fn forks_sessions_with_branch_metadata_and_persists_it() {
        let path = temp_session_path("fork");
        let mut session = Session::new();
        session
            .push_user_text("before fork")
            .expect("message should append");

        let forked = session
            .fork(Some("investigation".to_string()))
            .with_persistence_path(path.clone());
        forked
            .save_to_path(&path)
            .expect("forked session should save");

        let restored = Session::load_from_path(&path).expect("forked session should load");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_ne!(restored.session_id, session.session_id);
        assert_eq!(
            restored.fork,
            Some(SessionFork {
                parent_session_id: session.session_id,
                branch_name: Some("investigation".to_string()),
            })
        );
        assert_eq!(restored.messages, forked.messages);
    }

    #[test]
    fn rotates_and_cleans_up_large_session_logs() {
        // given
        let path = temp_session_path("rotation");
        let oversized_length =
            usize::try_from(super::ROTATE_AFTER_BYTES + 10).expect("rotate threshold should fit");
        fs::write(&path, "x".repeat(oversized_length)).expect("oversized file should write");

        // when
        rotate_session_file_if_needed(&path).expect("rotation should succeed");

        // then
        assert!(
            path.exists(),
            "live file must stay in place so a crash during rotation cannot lose the session"
        );
        assert_eq!(
            rotation_files(&path).len(),
            1,
            "one rotated copy should exist"
        );

        for seq in 1..=5 {
            let rotated = super::rotated_log_path(&path, seq);
            fs::write(&rotated, "old").expect("rotated file should write");
        }
        // With no uploader configured, only the newest copies are kept.
        cleanup_rotated_logs(&path).expect("cleanup should succeed");
        assert_eq!(rotation_files(&path).len(), super::MAX_ROTATED_FILES);

        // Once the uploader has confirmed everything, only the newest copies are kept.
        super::write_export_cursor(&path, 5).expect("cursor should write");
        cleanup_rotated_logs(&path).expect("cleanup should succeed");
        let rotated_count = rotation_files(&path).len();
        assert!(rotated_count <= super::MAX_ROTATED_FILES);
        for rotated in rotation_files(&path) {
            fs::remove_file(rotated).expect("rotated file should be removable");
        }
        let _ = fs::remove_file(super::export_cursor_path(&path));
    }

    #[test]
    fn unuploaded_rotated_copies_are_kept_while_under_the_byte_cap() {
        let path = temp_session_path("pending-cap");
        for seq in 1..=5 {
            fs::write(super::rotated_log_path(&path, seq), "old")
                .expect("rotated file should write");
        }
        // A cursor exists but has not reached any copy, so all five stay.
        super::write_export_cursor(&path, 0).expect("cursor should write");
        cleanup_rotated_logs(&path).expect("cleanup should succeed");
        assert_eq!(rotation_files(&path).len(), 5);

        for rotated in rotation_files(&path) {
            fs::remove_file(rotated).expect("rotated file should be removable");
        }
        let _ = fs::remove_file(super::export_cursor_path(&path));
    }

    #[cfg(unix)]
    #[test]
    fn session_and_rotated_files_are_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_session_path("private-mode");
        let mut session = Session::new();
        session
            .push_user_text("secret text")
            .expect("in-memory push should succeed");
        session.save_to_path(&path).expect("save should succeed");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        fs::remove_file(&path).expect("temp file should be removable");
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn rejects_jsonl_record_without_type() {
        // given
        let path = write_temp_session_file(
            "missing-type",
            r#"{"message":{"role":"user","blocks":[{"type":"text","text":"hello"}]}}"#,
        );

        // when
        let error = Session::load_from_path(&path)
            .expect_err("session should reject JSONL records without a type");

        // then
        assert!(error.to_string().contains("missing type"));
        fs::remove_file(path).expect("temp file should be removable");
    }

    #[test]
    fn rejects_jsonl_message_record_without_message_payload() {
        // given
        let path = write_temp_session_file("missing-message", r#"{"type":"message"}"#);

        // when
        let error = Session::load_from_path(&path)
            .expect_err("session should reject JSONL message records without message payload");

        // then
        assert!(error.to_string().contains("missing message"));
        fs::remove_file(path).expect("temp file should be removable");
    }

    #[test]
    fn rejects_jsonl_record_with_unknown_type() {
        // given
        let path = write_temp_session_file("unknown-type", r#"{"type":"mystery"}"#);

        // when
        let error = Session::load_from_path(&path)
            .expect_err("session should reject unknown JSONL record types");

        // then
        assert!(error.to_string().contains("unsupported JSONL record type"));
        fs::remove_file(path).expect("temp file should be removable");
    }

    #[test]
    fn rejects_legacy_session_json_without_messages() {
        // given
        let session = JsonValue::Object(
            [("version".to_string(), JsonValue::Number(1))]
                .into_iter()
                .collect(),
        );

        // when
        let error = Session::from_json(&session)
            .expect_err("legacy session objects should require messages");

        // then
        assert!(error.to_string().contains("missing messages"));
    }

    #[test]
    fn normalizes_blank_fork_branch_name_to_none() {
        // given
        let session = Session::new();

        // when
        let forked = session.fork(Some("   ".to_string()));

        // then
        assert_eq!(forked.fork.expect("fork metadata").branch_name, None);
    }

    #[test]
    fn rejects_unknown_content_block_type() {
        // given
        let block = JsonValue::Object(
            [("type".to_string(), JsonValue::String("unknown".to_string()))]
                .into_iter()
                .collect(),
        );

        // when
        let error = ContentBlock::from_json(&block)
            .expect_err("content blocks should reject unknown types");

        // then
        assert!(error.to_string().contains("unsupported block type"));
    }

    fn temp_session_path(label: &str) -> PathBuf {
        // Timestamp alone collides when parallel tests read the same instant.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let pid = std::process::id();
        let serial = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "runtime-session-{label}-{nanos}-{pid}-{serial}.json"
        ))
    }

    /// A version 1 JSONL file exactly as the previous build wrote it: no message identity,
    /// no sequence numbers, no session metadata.
    const VERSION_1_JSONL: &str = concat!(
        r#"{"type":"session_meta","version":1,"session_id":"session-legacy-1","created_at_ms":1000,"updated_at_ms":1001}"#,
        "\n",
        r#"{"type":"message","message":{"role":"user","blocks":[{"type":"text","text":"first"}]}}"#,
        "\n",
        r#"{"type":"message","message":{"role":"assistant","blocks":[{"type":"text","text":"second"}],"usage":{"input_tokens":3,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
    );

    #[test]
    fn version_1_jsonl_fixture_loads_with_positional_sequence_numbers() {
        let path = write_temp_session_file("v1-fixture", VERSION_1_JSONL.trim_end());

        let restored = Session::load_from_path(&path).expect("version 1 file should load");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored.session_id, "session-legacy-1");
        assert_eq!(restored.messages.len(), 2);
        assert_eq!(restored.messages[0].seq, Some(1));
        assert_eq!(restored.messages[1].seq, Some(2));
        assert_eq!(restored.next_seq, 3);
        // Identity is not invented on load, so re-exporting a legacy file is stable.
        assert_eq!(restored.messages[0].message_id, None);
        assert_eq!(restored.metadata, super::SessionMetadata::default());
    }

    #[test]
    fn version_1_file_is_upgraded_to_version_2_on_next_append() {
        let path = write_temp_session_file("v1-upgrade", VERSION_1_JSONL.trim_end());
        let mut restored = Session::load_from_path(&path).expect("version 1 file should load");

        restored
            .push_user_text("third")
            .expect("append should succeed");
        let reloaded = Session::load_from_path(&path).expect("upgraded file should load");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(reloaded.version, 2);
        assert_eq!(reloaded.messages.len(), 3);
        assert_eq!(reloaded.messages[0].seq, Some(1));
        assert_eq!(reloaded.messages[2].seq, Some(3));
        assert!(reloaded.messages[2].message_id.is_some());
        assert_eq!(reloaded.messages[2].parent_message_id, None);
    }

    #[test]
    fn rejects_session_version_newer_than_this_build() {
        let path = write_temp_session_file(
            "future-version",
            r#"{"type":"session_meta","version":3,"session_id":"s","created_at_ms":1,"updated_at_ms":1}"#,
        );

        let error = Session::load_from_path(&path).expect_err("future version should be rejected");
        fs::remove_file(&path).expect("temp file should be removable");

        assert!(error.to_string().contains("unsupported session version 3"));
    }

    #[test]
    fn pushed_messages_get_monotonic_seq_and_a_parent_chain() {
        let path = temp_session_path("identity-chain");
        let mut session = Session::new().with_persistence_path(path.clone());
        session
            .push_user_text("one")
            .expect("append should succeed");
        session
            .push_user_text("two")
            .expect("append should succeed");
        session
            .push_user_text("three")
            .expect("append should succeed");

        let restored = Session::load_from_path(&path).expect("session should reload");
        fs::remove_file(&path).expect("temp file should be removable");

        let seqs: Vec<_> = restored.messages.iter().map(|m| m.seq).collect();
        assert_eq!(seqs, vec![Some(1), Some(2), Some(3)]);
        let ids: Vec<_> = restored
            .messages
            .iter()
            .map(|m| m.message_id.clone().expect("pushed messages have ids"))
            .collect();
        assert_ne!(ids[0], ids[1]);
        assert_eq!(
            restored.messages[1].parent_message_id.as_deref(),
            Some(ids[0].as_str())
        );
        assert_eq!(
            restored.messages[2].parent_message_id.as_deref(),
            Some(ids[1].as_str())
        );
        assert!(restored.messages.iter().all(|m| m.created_at_ms.is_some()));
    }

    #[test]
    fn sequence_numbers_do_not_repeat_after_compaction_removes_the_tail() {
        let mut session = Session::new();
        session.messages = vec![ConversationMessage::user_text("a")];
        session.assign_missing_identity();
        assert_eq!(session.next_seq, 2);

        // Dropping every message must not reset the counter.
        session.messages.clear();
        session
            .push_user_text("b")
            .expect("in-memory push should succeed");
        assert_eq!(session.messages[0].seq, Some(2));
    }

    #[test]
    fn torn_last_line_from_a_crash_is_ignored_on_load() {
        let path = temp_session_path("torn-load");
        let complete: String = VERSION_1_JSONL.to_string();
        fs::write(&path, format!("{complete}{{\"type\":\"message\",\"mess")).expect("write");

        let restored = Session::load_from_path(&path).expect("torn tail should be tolerated");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored.messages.len(), 2);
    }

    #[test]
    fn corruption_in_the_middle_of_a_file_is_still_an_error() {
        let path = write_temp_session_file(
            "middle-corrupt",
            &format!(
                "{{\"type\":\"message\",\"mess\n{}",
                VERSION_1_JSONL.trim_end()
            ),
        );

        let error = Session::load_from_path(&path).expect_err("mid-file corruption must fail");
        fs::remove_file(&path).expect("temp file should be removable");

        assert!(error.to_string().contains("invalid JSONL record at line 1"));
    }

    #[test]
    fn append_after_a_torn_write_repairs_the_file_first() {
        let path = temp_session_path("torn-append");
        let mut session = Session::new().with_persistence_path(path.clone());
        session
            .push_user_text("kept")
            .expect("append should succeed");
        // Simulate a crash that left an unterminated fragment at the end.
        let mut contents = fs::read_to_string(&path).expect("file should read");
        contents.push_str("{\"type\":\"message\",\"mess");
        fs::write(&path, contents).expect("write");

        session
            .push_user_text("after crash")
            .expect("append should succeed");
        let restored = Session::load_from_path(&path).expect("repaired file should load");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored.messages.len(), 2);
        assert_eq!(
            restored.messages[1].blocks,
            ConversationMessage::user_text("after crash").blocks
        );
    }

    #[test]
    fn metadata_round_trips_through_jsonl() {
        let path = temp_session_path("metadata");
        let mut session = Session::new().with_persistence_path(path.clone());
        session.set_metadata(super::SessionMetadata {
            client: Some("cli".to_string()),
            client_version: Some("1.2.3".to_string()),
            repo_id: Some("sha256:abc".to_string()),
            branch: Some("main".to_string()),
            model: Some("model-a".to_string()),
            provider: Some("anthropic".to_string()),
            ..super::SessionMetadata::default()
        });
        session
            .push_user_text("hello")
            .expect("append should succeed");

        let restored = Session::load_from_path(&path).expect("session should reload");
        fs::remove_file(&path).expect("temp file should be removable");

        assert_eq!(restored.metadata.client.as_deref(), Some("cli"));
        assert_eq!(restored.metadata.repo_id.as_deref(), Some("sha256:abc"));
        assert_eq!(restored.metadata.user_id, None);
    }

    #[test]
    fn repo_id_ignores_embedded_credentials() {
        let with_token = super::SessionMetadata::repo_id_for_remote(
            "https://x-access-token:ghp_secret123@github.com/frontal-labs/code.git",
        );
        let plain =
            super::SessionMetadata::repo_id_for_remote("https://github.com/frontal-labs/code.git");

        assert_eq!(with_token, plain);
        assert!(!with_token.contains("ghp_secret123"));
    }

    #[test]
    fn repo_id_is_stable_across_url_spelling_and_never_contains_the_url() {
        let https =
            super::SessionMetadata::repo_id_for_remote("https://GitHub.com/frontal-labs/code.git");
        let trailing =
            super::SessionMetadata::repo_id_for_remote("https://github.com/frontal-labs/code/");
        let other =
            super::SessionMetadata::repo_id_for_remote("https://github.com/frontal-labs/other.git");

        assert_eq!(
            https, trailing,
            "trailing slash and .git must not change the id"
        );
        assert_ne!(https, other);
        assert!(https.starts_with("sha256:"));
        assert!(!https.contains("github"));
    }

    #[test]
    fn set_metadata_keeps_fields_the_update_does_not_set() {
        let mut session = Session::new();
        session.set_metadata(super::SessionMetadata {
            branch: Some("main".to_string()),
            ..super::SessionMetadata::default()
        });
        session.set_metadata(super::SessionMetadata {
            model: Some("model-b".to_string()),
            ..super::SessionMetadata::default()
        });

        assert_eq!(session.metadata.branch.as_deref(), Some("main"));
        assert_eq!(session.metadata.model.as_deref(), Some("model-b"));
    }

    #[test]
    fn compaction_archives_removed_messages_before_replacing_them() {
        let path = temp_session_path("archive-ok");
        let mut session = Session::new().with_persistence_path(path.clone());
        for text in ["one", "two", "three", "four", "five"] {
            session.push_user_text(text).expect("append should succeed");
        }
        let originals = session.messages.clone();
        let result = crate::compact::compact_session(
            &session,
            crate::compact::CompactionConfig {
                preserve_recent_messages: 2,
                max_estimated_tokens: 0,
            },
        );
        let removed_count = result.removed_messages.len();

        session
            .apply_compaction(
                result.compacted_session,
                &result.removed_messages,
                Some(&path),
            )
            .expect("compaction should apply");

        let archive_path = path
            .parent()
            .expect("parent")
            .join("archive")
            .join(format!("{}.jsonl", super::file_stem_or_default(&path)));
        let archive = fs::read_to_string(&archive_path).expect("archive should exist");
        let restored = Session::load_from_path(&path).expect("compacted file should reload");
        fs::remove_file(&path).expect("temp file should be removable");
        fs::remove_file(&archive_path).expect("archive should be removable");

        assert_eq!(removed_count, 3);
        assert_eq!(archive.lines().count(), removed_count);
        for original in originals.iter().take(removed_count) {
            let id = original.message_id.as_deref().expect("original has id");
            assert!(
                archive.contains(id),
                "archive must hold removed message {id}"
            );
        }
        // Preserved messages keep their sequence numbers.
        assert_eq!(restored.messages.last().and_then(|m| m.seq), Some(5));
        assert_eq!(restored.compaction.as_ref().map(|c| c.count), Some(1));
    }

    #[test]
    fn failed_archive_leaves_the_full_history_in_place() {
        let path = temp_session_path("archive-fails");
        let mut session = Session::new().with_persistence_path(path.clone());
        for text in ["one", "two", "three", "four"] {
            session.push_user_text(text).expect("append should succeed");
        }
        let before = session.messages.clone();
        let result = crate::compact::compact_session(
            &session,
            crate::compact::CompactionConfig {
                preserve_recent_messages: 1,
                max_estimated_tokens: 0,
            },
        );
        // A directory where the archive file should be makes the archive step fail.
        let archive_dir = path.parent().expect("parent").join("archive");
        fs::create_dir_all(&archive_dir).expect("archive dir should create");
        let archive_file =
            archive_dir.join(format!("{}.jsonl", super::file_stem_or_default(&path)));
        fs::create_dir_all(&archive_file).expect("blocker should create");

        let error = session.apply_compaction(
            result.compacted_session,
            &result.removed_messages,
            Some(&path),
        );
        let on_disk = Session::load_from_path(&path).expect("file should still load");
        fs::remove_dir_all(&archive_file).expect("blocker should be removable");
        fs::remove_file(&path).expect("temp file should be removable");

        assert!(error.is_err());
        assert_eq!(session.messages, before);
        assert_eq!(on_disk.messages.len(), 4);
    }

    #[test]
    fn export_cursor_round_trips_and_defaults_to_absent() {
        let path = temp_session_path("cursor");
        assert_eq!(super::read_export_cursor(&path), None);

        super::write_export_cursor(&path, 42).expect("cursor should write");
        assert_eq!(super::read_export_cursor(&path), Some(42));
        fs::remove_file(super::export_cursor_path(&path)).expect("cursor should be removable");
    }

    #[test]
    fn successful_save_leaves_no_temporary_files_behind() {
        let path = temp_session_path("no-temp");
        let mut session = Session::new();
        session
            .push_user_text("x")
            .expect("in-memory push should succeed");
        session.save_to_path(&path).expect("save should succeed");

        let stem = super::file_stem_or_default(&path).to_string();
        let leftovers: Vec<_> = fs::read_dir(path.parent().expect("parent"))
            .expect("dir should read")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{stem}.tmp-"))
            })
            .collect();
        fs::remove_file(&path).expect("temp file should be removable");
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
    }

    fn write_temp_session_file(label: &str, contents: &str) -> PathBuf {
        let path = temp_session_path(label);
        fs::write(&path, format!("{contents}\n")).expect("temp session file should write");
        path
    }

    fn rotation_files(path: &Path) -> Vec<PathBuf> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .expect("temp path should have file stem")
            .to_string();
        fs::read_dir(path.parent().expect("temp path should have parent"))
            .expect("temp dir should read")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|entry_path| {
                entry_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| {
                        name.starts_with(&format!("{stem}.rot-"))
                            && Path::new(name)
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
                    })
            })
            .collect()
    }
}
