use std::cmp::Reverse;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use glob::Pattern;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/// Maximum file size that can be read (10 MB).
const MAX_READ_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum file size that can be written (10 MB).
const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024;

/// Check whether a file appears to contain binary content by examining
/// the first chunk for NUL bytes.
fn is_binary_file(path: &Path) -> io::Result<bool> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 8192];
    let bytes_read = file.read(&mut buffer)?;
    Ok(buffer[..bytes_read].contains(&0))
}

/// Validate that a resolved path stays within the given workspace root.
/// Returns the canonical path on success, or an error if the path escapes
/// the workspace boundary (e.g. via `../` traversal or symlink).
fn validate_workspace_boundary(resolved: &Path, workspace_root: &Path) -> io::Result<()> {
    if !resolved.starts_with(workspace_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "path {} escapes workspace boundary {}",
                resolved.display(),
                workspace_root.display()
            ),
        ));
    }
    Ok(())
}

/// Text payload returned by file-reading operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextFilePayload {
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub content: String,
    #[serde(rename = "numLines")]
    pub num_lines: usize,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    #[serde(rename = "totalLines")]
    pub total_lines: usize,
}

/// Output envelope for the `read_file` tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadFileOutput {
    #[serde(rename = "type")]
    pub kind: String,
    pub file: TextFilePayload,
}

/// Structured patch hunk emitted by write and edit operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredPatchHunk {
    #[serde(rename = "oldStart")]
    pub old_start: usize,
    #[serde(rename = "oldLines")]
    pub old_lines: usize,
    #[serde(rename = "newStart")]
    pub new_start: usize,
    #[serde(rename = "newLines")]
    pub new_lines: usize,
    pub lines: Vec<String>,
}

/// Output envelope for full-file write operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteFileOutput {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub content: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "originalFile")]
    pub original_file: Option<String>,
    #[serde(rename = "gitDiff")]
    pub git_diff: Option<serde_json::Value>,
}

/// Output envelope for targeted string-replacement edits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditFileOutput {
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(rename = "oldString")]
    pub old_string: String,
    #[serde(rename = "newString")]
    pub new_string: String,
    #[serde(rename = "originalFile")]
    pub original_file: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "userModified")]
    pub user_modified: bool,
    #[serde(rename = "replaceAll")]
    pub replace_all: bool,
    #[serde(rename = "gitDiff")]
    pub git_diff: Option<serde_json::Value>,
}

/// Result of a glob-based filename search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobSearchOutput {
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    pub filenames: Vec<String>,
    pub truncated: bool,
}

/// Parameters accepted by the grep-style search tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    #[serde(rename = "output_mode")]
    pub output_mode: Option<String>,
    #[serde(rename = "-B")]
    pub before: Option<usize>,
    #[serde(rename = "-A")]
    pub after: Option<usize>,
    #[serde(rename = "-C")]
    pub context_short: Option<usize>,
    pub context: Option<usize>,
    #[serde(rename = "-n")]
    pub line_numbers: Option<bool>,
    #[serde(rename = "-i")]
    pub case_insensitive: Option<bool>,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub head_limit: Option<usize>,
    pub offset: Option<usize>,
    pub multiline: Option<bool>,
}

/// Result payload returned by the grep-style search tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrepSearchOutput {
    pub mode: Option<String>,
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    pub filenames: Vec<String>,
    pub content: Option<String>,
    #[serde(rename = "numLines")]
    pub num_lines: Option<usize>,
    #[serde(rename = "numMatches")]
    pub num_matches: Option<usize>,
    #[serde(rename = "appliedLimit")]
    pub applied_limit: Option<usize>,
    #[serde(rename = "appliedOffset")]
    pub applied_offset: Option<usize>,
}

/// Reads a text file and returns a line-windowed payload.
pub fn read_file(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> io::Result<ReadFileOutput> {
    let absolute_path = normalize_path(path)?;
    read_resolved(&absolute_path, offset, limit)
}

/// Reads an already-resolved (canonical) path.
fn read_resolved(
    absolute_path: &Path,
    offset: Option<usize>,
    limit: Option<usize>,
) -> io::Result<ReadFileOutput> {
    // Check file size before reading
    let metadata = fs::metadata(absolute_path)?;
    if metadata.len() > MAX_READ_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "file is too large ({} bytes, max {} bytes)",
                metadata.len(),
                MAX_READ_SIZE
            ),
        ));
    }

    // Detect binary files
    if is_binary_file(absolute_path)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file appears to be binary",
        ));
    }

    let content = fs::read_to_string(absolute_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start_index = offset.unwrap_or(0).min(lines.len());
    let end_index = limit.map_or(lines.len(), |limit| {
        start_index.saturating_add(limit).min(lines.len())
    });
    let selected = lines[start_index..end_index].join("\n");

    Ok(ReadFileOutput {
        kind: String::from("text"),
        file: TextFilePayload {
            file_path: absolute_path.to_string_lossy().into_owned(),
            content: selected,
            num_lines: end_index.saturating_sub(start_index),
            start_line: start_index.saturating_add(1),
            total_lines: lines.len(),
        },
    })
}

/// Replaces a file's contents and returns patch metadata.
pub fn write_file(path: &str, content: &str) -> io::Result<WriteFileOutput> {
    check_write_size(content)?;
    let absolute_path = normalize_path_allow_missing(path)?;
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_resolved(&absolute_path, content, false)
}

fn check_write_size(content: &str) -> io::Result<()> {
    if content.len() > MAX_WRITE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "content is too large ({} bytes, max {} bytes)",
                content.len(),
                MAX_WRITE_SIZE
            ),
        ));
    }
    Ok(())
}

/// `O_NOFOLLOW` for the platforms this is built on, so an open never follows a symlink
/// at the final path component. Other platforms rely on the `symlink_metadata` check.
#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0x20000;
#[cfg(target_os = "macos")]
const O_NOFOLLOW: i32 = 0x0100;

/// Writes `content` to `path`, which must already be resolved. When `refuse_symlink` is
/// set, a symlink at the final component is an error rather than a write target.
fn write_resolved(path: &Path, content: &str, refuse_symlink: bool) -> io::Result<WriteFileOutput> {
    let original_file = fs::read_to_string(path).ok();
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    if refuse_symlink {
        if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to write through symlink {}", path.display()),
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(O_NOFOLLOW);
        }
    }
    let mut file = options.open(path)?;
    io::Write::write_all(&mut file, content.as_bytes())?;
    let absolute_path = path.to_path_buf();

    Ok(WriteFileOutput {
        kind: if original_file.is_some() {
            String::from("update")
        } else {
            String::from("create")
        },
        file_path: absolute_path.to_string_lossy().into_owned(),
        content: content.to_owned(),
        structured_patch: make_patch(original_file.as_deref().unwrap_or(""), content),
        original_file,
        git_diff: None,
    })
}

/// Performs an in-file string replacement and returns patch metadata.
pub fn edit_file(
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> io::Result<EditFileOutput> {
    let absolute_path = normalize_path(path)?;
    edit_resolved(&absolute_path, old_string, new_string, replace_all)
}

/// Edits an already-resolved (canonical) path.
fn edit_resolved(
    absolute_path: &Path,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> io::Result<EditFileOutput> {
    let original_file = fs::read_to_string(absolute_path)?;
    if old_string == new_string {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "old_string and new_string must differ",
        ));
    }
    if !original_file.contains(old_string) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "old_string not found in file",
        ));
    }

    let updated = if replace_all {
        original_file.replace(old_string, new_string)
    } else {
        original_file.replacen(old_string, new_string, 1)
    };
    fs::write(absolute_path, &updated)?;

    Ok(EditFileOutput {
        file_path: absolute_path.to_string_lossy().into_owned(),
        old_string: old_string.to_owned(),
        new_string: new_string.to_owned(),
        original_file: original_file.clone(),
        structured_patch: make_patch(&original_file, &updated),
        user_modified: false,
        replace_all,
        git_diff: None,
    })
}

/// Expands a glob pattern and returns matching filenames.
pub fn glob_search(pattern: &str, path: Option<&str>) -> io::Result<GlobSearchOutput> {
    let base_dir = path
        .map(normalize_path)
        .transpose()?
        .unwrap_or(std::env::current_dir()?);
    glob_in(pattern, &base_dir, None)
}

/// Shared glob implementation. When `workspace_root` is set, matches whose canonical
/// location falls outside that root (for example through `../` or a symlink) are dropped.
fn glob_in(
    pattern: &str,
    base_dir: &Path,
    workspace_root: Option<&Path>,
) -> io::Result<GlobSearchOutput> {
    let started = Instant::now();
    let search_pattern = if Path::new(pattern).is_absolute() {
        pattern.to_owned()
    } else {
        base_dir.join(pattern).to_string_lossy().into_owned()
    };

    let mut matches = Vec::new();
    let entries = glob::glob(&search_pattern)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    for entry in entries.flatten() {
        if !entry.is_file() {
            continue;
        }
        if let Some(root) = workspace_root {
            let Ok(canonical) = entry.canonicalize() else {
                continue;
            };
            if !canonical.starts_with(root) {
                continue;
            }
        }
        matches.push(entry);
    }

    matches.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(Reverse)
    });

    let truncated = matches.len() > 100;
    let filenames = matches
        .into_iter()
        .take(100)
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    Ok(GlobSearchOutput {
        duration_ms: started.elapsed().as_millis(),
        num_files: filenames.len(),
        filenames,
        truncated,
    })
}

/// Runs a regex search over workspace files with optional context lines.
pub fn grep_search(input: &GrepSearchInput) -> io::Result<GrepSearchOutput> {
    let base_path = input
        .path
        .as_deref()
        .map(normalize_path)
        .transpose()?
        .unwrap_or(std::env::current_dir()?);
    grep_in(input, &base_path)
}

fn grep_in(input: &GrepSearchInput, base_path: &Path) -> io::Result<GrepSearchOutput> {
    let regex = RegexBuilder::new(&input.pattern)
        .case_insensitive(input.case_insensitive.unwrap_or(false))
        .dot_matches_new_line(input.multiline.unwrap_or(false))
        .build()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;

    let glob_filter = input
        .glob
        .as_deref()
        .map(Pattern::new)
        .transpose()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let file_type = input.file_type.as_deref();
    let output_mode = input
        .output_mode
        .clone()
        .unwrap_or_else(|| String::from("files_with_matches"));
    let context = input.context.or(input.context_short).unwrap_or(0);

    let mut filenames = Vec::new();
    let mut content_lines = Vec::new();
    let mut total_matches = 0usize;

    for file_path in collect_search_files(base_path)? {
        if !matches_optional_filters(&file_path, glob_filter.as_ref(), file_type) {
            continue;
        }

        let Ok(file_contents) = fs::read_to_string(&file_path) else {
            continue;
        };

        if output_mode == "count" {
            let count = regex.find_iter(&file_contents).count();
            if count > 0 {
                filenames.push(file_path.to_string_lossy().into_owned());
                total_matches += count;
            }
            continue;
        }

        let lines: Vec<&str> = file_contents.lines().collect();
        let mut matched_lines = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                total_matches += 1;
                matched_lines.push(index);
            }
        }

        if matched_lines.is_empty() {
            continue;
        }

        filenames.push(file_path.to_string_lossy().into_owned());
        if output_mode == "content" {
            for index in matched_lines {
                let start = index.saturating_sub(input.before.unwrap_or(context));
                let end = (index + input.after.unwrap_or(context) + 1).min(lines.len());
                for (current, line) in lines.iter().enumerate().take(end).skip(start) {
                    let prefix = if input.line_numbers.unwrap_or(true) {
                        format!("{}:{}:", file_path.to_string_lossy(), current + 1)
                    } else {
                        format!("{}:", file_path.to_string_lossy())
                    };
                    content_lines.push(format!("{prefix}{line}"));
                }
            }
        }
    }

    let (filenames, applied_limit, applied_offset) =
        apply_limit(filenames, input.head_limit, input.offset);
    let content_output = if output_mode == "content" {
        let (lines, limit, offset) = apply_limit(content_lines, input.head_limit, input.offset);
        return Ok(GrepSearchOutput {
            mode: Some(output_mode),
            num_files: filenames.len(),
            filenames,
            num_lines: Some(lines.len()),
            content: Some(lines.join("\n")),
            num_matches: None,
            applied_limit: limit,
            applied_offset: offset,
        });
    } else {
        None
    };

    Ok(GrepSearchOutput {
        mode: Some(output_mode.clone()),
        num_files: filenames.len(),
        filenames,
        content: content_output,
        num_lines: None,
        num_matches: (output_mode == "count").then_some(total_matches),
        applied_limit,
        applied_offset,
    })
}

fn collect_search_files(base_path: &Path) -> io::Result<Vec<PathBuf>> {
    if base_path.is_file() {
        return Ok(vec![base_path.to_path_buf()]);
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(base_path) {
        let entry = entry.map_err(|error| io::Error::other(error.to_string()))?;
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

fn matches_optional_filters(
    path: &Path,
    glob_filter: Option<&Pattern>,
    file_type: Option<&str>,
) -> bool {
    if let Some(glob_filter) = glob_filter {
        let path_string = path.to_string_lossy();
        if !glob_filter.matches(&path_string) && !glob_filter.matches_path(path) {
            return false;
        }
    }

    if let Some(file_type) = file_type {
        let extension = path.extension().and_then(|extension| extension.to_str());
        if extension != Some(file_type) {
            return false;
        }
    }

    true
}

fn apply_limit<T>(
    items: Vec<T>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> (Vec<T>, Option<usize>, Option<usize>) {
    let offset_value = offset.unwrap_or(0);
    let mut items = items.into_iter().skip(offset_value).collect::<Vec<_>>();
    let explicit_limit = limit.unwrap_or(250);
    if explicit_limit == 0 {
        return (items, None, (offset_value > 0).then_some(offset_value));
    }

    let truncated = items.len() > explicit_limit;
    items.truncate(explicit_limit);
    (
        items,
        truncated.then_some(explicit_limit),
        (offset_value > 0).then_some(offset_value),
    )
}

fn make_patch(original: &str, updated: &str) -> Vec<StructuredPatchHunk> {
    let mut lines = Vec::new();
    for line in original.lines() {
        lines.push(format!("-{line}"));
    }
    for line in updated.lines() {
        lines.push(format!("+{line}"));
    }

    vec![StructuredPatchHunk {
        old_start: 1,
        old_lines: original.lines().count(),
        new_start: 1,
        new_lines: updated.lines().count(),
        lines,
    }]
}

fn normalize_path(path: &str) -> io::Result<PathBuf> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };
    candidate.canonicalize()
}

fn normalize_path_allow_missing(path: &str) -> io::Result<PathBuf> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };

    if let Ok(canonical) = candidate.canonicalize() {
        return Ok(canonical);
    }

    if let Some(parent) = candidate.parent() {
        let canonical_parent = parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf());
        if let Some(name) = candidate.file_name() {
            return Ok(canonical_parent.join(name));
        }
    }

    Ok(candidate)
}

/// Resolves a write target that may not exist yet. The deepest existing prefix of the path is
/// canonicalized, which resolves every symlink in it. The components after that prefix do not
/// exist, so `.` and `..` are applied to them lexically. The returned path is the location the
/// write will target, so callers validate exactly what is written.
fn resolve_for_write(path: &str) -> io::Result<PathBuf> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };
    let components: Vec<Component<'_>> = candidate.components().collect();

    // `symlink_metadata` treats a dangling symlink as existing. canonicalize then fails for it,
    // so the write is refused rather than creating through the link.
    let mut split = components.len();
    while split > 0 {
        let prefix: PathBuf = components[..split].iter().collect();
        if fs::symlink_metadata(&prefix).is_ok() {
            break;
        }
        split -= 1;
    }

    let mut resolved = components[..split]
        .iter()
        .collect::<PathBuf>()
        .canonicalize()?;
    for component in &components[split..] {
        match component {
            Component::Normal(name) => resolved.push(name),
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unexpected root component in write path",
                ));
            }
        }
    }
    Ok(resolved)
}

/// Resolves an existing path (or the current directory when `path` is `None`) and checks that
/// it lies inside `canonical_root`. Symlinks are resolved first, so a link pointing outside the
/// root is rejected.
fn resolve_existing_in_workspace(path: Option<&str>, canonical_root: &Path) -> io::Result<PathBuf> {
    let resolved = match path {
        Some(path) => normalize_path(path)?,
        None => std::env::current_dir()?.canonicalize()?,
    };
    validate_workspace_boundary(&resolved, canonical_root)?;
    Ok(resolved)
}

/// Read a file with explicit workspace boundary enforcement.
pub fn read_file_in_workspace(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
    workspace_root: &Path,
) -> io::Result<ReadFileOutput> {
    let canonical_root = workspace_root.canonicalize()?;
    let resolved = resolve_existing_in_workspace(Some(path), &canonical_root)?;
    read_resolved(&resolved, offset, limit)
}

/// Write a file with explicit workspace boundary enforcement. Parent directories are created
/// only after the target is validated, and the write refuses a symlink at the final component.
pub fn write_file_in_workspace(
    path: &str,
    content: &str,
    workspace_root: &Path,
) -> io::Result<WriteFileOutput> {
    check_write_size(content)?;
    let canonical_root = workspace_root.canonicalize()?;
    let target = resolve_for_write(path)?;
    validate_workspace_boundary(&target, &canonical_root)?;

    let parent = target.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    fs::create_dir_all(parent)?;
    // Re-check after creating directories: the resolved parent must still be inside the root.
    let canonical_parent = parent.canonicalize()?;
    validate_workspace_boundary(&canonical_parent, &canonical_root)?;
    write_resolved(&canonical_parent.join(file_name), content, true)
}

/// Edit a file with explicit workspace boundary enforcement.
pub fn edit_file_in_workspace(
    path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    workspace_root: &Path,
) -> io::Result<EditFileOutput> {
    let canonical_root = workspace_root.canonicalize()?;
    let resolved = resolve_existing_in_workspace(Some(path), &canonical_root)?;
    edit_resolved(&resolved, old_string, new_string, replace_all)
}

/// Glob with explicit workspace boundary enforcement. The search base must lie inside the
/// workspace, and matches that resolve outside it (via `..` or symlinks) are dropped.
pub fn glob_search_in_workspace(
    pattern: &str,
    path: Option<&str>,
    workspace_root: &Path,
) -> io::Result<GlobSearchOutput> {
    let canonical_root = workspace_root.canonicalize()?;
    let base_dir = resolve_existing_in_workspace(path, &canonical_root)?;
    glob_in(pattern, &base_dir, Some(&canonical_root))
}

/// Grep with explicit workspace boundary enforcement. The search base must lie inside the
/// workspace. The walk does not follow symlinks, so linked files outside it are not read.
pub fn grep_search_in_workspace(
    input: &GrepSearchInput,
    workspace_root: &Path,
) -> io::Result<GrepSearchOutput> {
    let canonical_root = workspace_root.canonicalize()?;
    let base_path = resolve_existing_in_workspace(input.path.as_deref(), &canonical_root)?;
    grep_in(input, &base_path)
}

/// Check whether a path is a symlink that resolves outside the workspace.
pub fn is_symlink_escape(path: &Path, workspace_root: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_symlink() {
        return Ok(false);
    }
    let resolved = path.canonicalize()?;
    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    Ok(!resolved.starts_with(&canonical_root))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        edit_file, glob_search, grep_search, is_symlink_escape, read_file, read_file_in_workspace,
        write_file, GrepSearchInput, MAX_WRITE_SIZE,
    };

    fn temp_path(name: &str) -> std::path::PathBuf {
        // Timestamp alone collides when parallel tests read the same instant.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let pid = std::process::id();
        let serial = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "frontal-code-native-{name}-{unique}-{pid}-{serial}"
        ))
    }

    #[test]
    fn reads_and_writes_files() {
        let path = temp_path("read-write.txt");
        let write_output = write_file(path.to_string_lossy().as_ref(), "one\ntwo\nthree")
            .expect("write should succeed");
        assert_eq!(write_output.kind, "create");

        let read_output = read_file(path.to_string_lossy().as_ref(), Some(1), Some(1))
            .expect("read should succeed");
        assert_eq!(read_output.file.content, "two");
    }

    #[test]
    fn edits_file_contents() {
        let path = temp_path("edit.txt");
        write_file(path.to_string_lossy().as_ref(), "alpha beta alpha")
            .expect("initial write should succeed");
        let output = edit_file(path.to_string_lossy().as_ref(), "alpha", "omega", true)
            .expect("edit should succeed");
        assert!(output.replace_all);
    }

    #[test]
    fn rejects_binary_files() {
        let path = temp_path("binary-test.bin");
        std::fs::write(&path, b"\x00\x01\x02\x03binary content").expect("write should succeed");
        let result = read_file(path.to_string_lossy().as_ref(), None, None);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("binary"));
    }

    #[test]
    fn rejects_oversized_writes() {
        let path = temp_path("oversize-write.txt");
        let huge = "x".repeat(MAX_WRITE_SIZE + 1);
        let result = write_file(path.to_string_lossy().as_ref(), &huge);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("too large"));
    }

    #[test]
    fn enforces_workspace_boundary() {
        let workspace = temp_path("workspace-boundary");
        std::fs::create_dir_all(&workspace).expect("workspace dir should be created");
        let inside = workspace.join("inside.txt");
        write_file(inside.to_string_lossy().as_ref(), "safe content")
            .expect("write inside workspace should succeed");

        // Reading inside workspace should succeed
        let result =
            read_file_in_workspace(inside.to_string_lossy().as_ref(), None, None, &workspace);
        assert!(result.is_ok());

        // Reading outside workspace should fail
        let outside = temp_path("outside-boundary.txt");
        write_file(outside.to_string_lossy().as_ref(), "unsafe content")
            .expect("write outside should succeed");
        let result =
            read_file_in_workspace(outside.to_string_lossy().as_ref(), None, None, &workspace);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("escapes workspace"));
    }

    #[test]
    fn detects_symlink_escape() {
        let workspace = temp_path("symlink-workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir should be created");
        let outside = temp_path("symlink-target.txt");
        std::fs::write(&outside, "target content").expect("target should write");

        let link_path = workspace.join("escape-link.txt");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &link_path).expect("symlink should create");
            assert!(is_symlink_escape(&link_path, &workspace).expect("check should succeed"));
        }

        // Non-symlink file should not be an escape
        let normal = workspace.join("normal.txt");
        std::fs::write(&normal, "normal content").expect("normal file should write");
        assert!(!is_symlink_escape(&normal, &workspace).expect("check should succeed"));
    }

    #[test]
    fn globs_and_greps_directory() {
        let dir = temp_path("search-dir");
        std::fs::create_dir_all(&dir).expect("directory should be created");
        let file = dir.join("demo.rs");
        write_file(
            file.to_string_lossy().as_ref(),
            "fn main() {\n println!(\"hello\");\n}\n",
        )
        .expect("file write should succeed");

        let globbed = glob_search("**/*.rs", Some(dir.to_string_lossy().as_ref()))
            .expect("glob should succeed");
        assert_eq!(globbed.num_files, 1);

        let grep_output = grep_search(&GrepSearchInput {
            pattern: String::from("hello"),
            path: Some(dir.to_string_lossy().into_owned()),
            glob: Some(String::from("**/*.rs")),
            output_mode: Some(String::from("content")),
            before: None,
            after: None,
            context_short: None,
            context: None,
            line_numbers: Some(true),
            case_insensitive: Some(false),
            file_type: None,
            head_limit: Some(10),
            offset: Some(0),
            multiline: Some(false),
        })
        .expect("grep should succeed");
        assert!(grep_output.content.unwrap_or_default().contains("hello"));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_write_refuses_symlink_at_final_component() {
        let root = std::env::temp_dir().join(format!("file-ops-symlink-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("file-ops-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).expect("workspace");
        std::fs::create_dir_all(&outside).expect("outside");
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, "original").expect("victim");
        std::os::unix::fs::symlink(&victim, root.join("link.txt")).expect("symlink");

        let error = super::write_file_in_workspace("link.txt", "pwned", &root)
            .expect_err("write through a symlink should be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read_to_string(&victim).expect("read victim"),
            "original"
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
        std::fs::remove_dir_all(&outside).expect("cleanup outside");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_write_does_not_create_directories_outside_through_symlink() {
        let root = std::env::temp_dir().join(format!("file-ops-mkdir-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("file-ops-mkdir-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).expect("workspace");
        std::fs::create_dir_all(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");

        let result = super::write_file_in_workspace("link/newdir/file.txt", "x", &root);
        assert!(
            result.is_err(),
            "write through a symlinked directory should be refused"
        );
        assert!(
            !outside.join("newdir").exists(),
            "no directory may be created outside"
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
        std::fs::remove_dir_all(&outside).expect("cleanup outside");
    }

    /// Creates a workspace directory and a sibling "outside" directory holding `secret.txt`.
    /// Both live under the temp dir with a unique name so parallel tests do not collide.
    fn boundary_fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "file-ops-boundary-{name}-{}-{unique}",
            std::process::id()
        ));
        let root = base.join("workspace");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).expect("workspace");
        std::fs::create_dir_all(&outside).expect("outside");
        std::fs::write(root.join("inside.txt"), "inside needle\n").expect("inside file");
        std::fs::write(outside.join("secret.txt"), "secret needle\n").expect("secret file");
        (root, outside)
    }

    fn grep_input(pattern: &str, path: Option<String>) -> super::GrepSearchInput {
        super::GrepSearchInput {
            pattern: pattern.to_string(),
            path,
            glob: None,
            output_mode: Some("content".to_string()),
            before: None,
            after: None,
            context_short: None,
            context: None,
            line_numbers: None,
            case_insensitive: None,
            file_type: None,
            head_limit: None,
            offset: None,
            multiline: None,
        }
    }

    #[test]
    fn workspace_tools_reject_absolute_path_outside_workspace() {
        let (root, outside) = boundary_fixture("absolute");
        let secret = outside.join("secret.txt");
        let secret_str = secret.to_string_lossy().into_owned();
        let new_file = outside.join("planted.txt");

        let read = super::read_file_in_workspace(&secret_str, None, None, &root)
            .expect_err("read outside should be refused");
        assert_eq!(read.kind(), std::io::ErrorKind::PermissionDenied);

        let write = super::write_file_in_workspace(&new_file.to_string_lossy(), "pwned", &root)
            .expect_err("write outside should be refused");
        assert_eq!(write.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!new_file.exists());

        let edit = super::edit_file_in_workspace(&secret_str, "secret", "pwned", false, &root)
            .expect_err("edit outside should be refused");
        assert_eq!(edit.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read_to_string(&secret).expect("read secret"),
            "secret needle\n"
        );

        let outside_dir = outside.to_string_lossy().into_owned();
        super::glob_search_in_workspace("*.txt", Some(&outside_dir), &root)
            .expect_err("glob base outside should be refused");
        super::grep_search_in_workspace(&grep_input("needle", Some(outside_dir)), &root)
            .expect_err("grep base outside should be refused");

        std::fs::remove_dir_all(root.parent().expect("base")).expect("cleanup");
    }

    #[test]
    fn workspace_tools_reject_dotdot_traversal_out_of_workspace() {
        let (root, outside) = boundary_fixture("dotdot");
        let root_str = root.to_string_lossy().into_owned();
        let outside_name = outside
            .file_name()
            .expect("outside name")
            .to_string_lossy()
            .into_owned();
        let secret_traversal = format!("{root_str}/../{outside_name}/secret.txt");
        let write_traversal = format!("{root_str}/../{outside_name}/planted.txt");
        let write_through_missing = format!("{root_str}/missing/../../{outside_name}/planted.txt");

        let read = super::read_file_in_workspace(&secret_traversal, None, None, &root)
            .expect_err("read via ../ should be refused");
        assert_eq!(read.kind(), std::io::ErrorKind::PermissionDenied);

        let edit =
            super::edit_file_in_workspace(&secret_traversal, "secret", "pwned", false, &root)
                .expect_err("edit via ../ should be refused");
        assert_eq!(edit.kind(), std::io::ErrorKind::PermissionDenied);

        let write = super::write_file_in_workspace(&write_traversal, "pwned", &root)
            .expect_err("write via ../ should be refused");
        assert_eq!(write.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!outside.join("planted.txt").exists());

        let write_missing = super::write_file_in_workspace(&write_through_missing, "pwned", &root)
            .expect_err("write via missing/../../ should be refused");
        assert_eq!(write_missing.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!outside.join("planted.txt").exists());

        std::fs::remove_dir_all(root.parent().expect("base")).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_tools_reject_symlink_escape() {
        let (root, outside) = boundary_fixture("symlink");
        let at = |rel: &str| root.join(rel).to_string_lossy().into_owned();
        let secret = outside.join("secret.txt");
        std::os::unix::fs::symlink(&outside, root.join("linkdir")).expect("dir symlink");
        std::os::unix::fs::symlink(&secret, root.join("linkfile.txt")).expect("file symlink");
        std::os::unix::fs::symlink(outside.join("new.txt"), root.join("dangling.txt"))
            .expect("dangling symlink");

        // Read through a symlinked directory and a symlinked file.
        let read_dir = super::read_file_in_workspace(&at("linkdir/secret.txt"), None, None, &root)
            .expect_err("read through dir symlink should be refused");
        assert_eq!(read_dir.kind(), std::io::ErrorKind::PermissionDenied);
        let read_file = super::read_file_in_workspace(&at("linkfile.txt"), None, None, &root)
            .expect_err("read through file symlink should be refused");
        assert_eq!(read_file.kind(), std::io::ErrorKind::PermissionDenied);

        // Write through a symlinked directory, onto a symlinked file, and onto a dangling link.
        let write_dir = super::write_file_in_workspace(&at("linkdir/planted.txt"), "pwned", &root)
            .expect_err("write through dir symlink should be refused");
        assert_eq!(write_dir.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!outside.join("planted.txt").exists());
        super::write_file_in_workspace(&at("linkfile.txt"), "pwned", &root)
            .expect_err("write through file symlink should be refused");
        super::write_file_in_workspace(&at("dangling.txt"), "pwned", &root)
            .expect_err("write through dangling symlink should be refused");
        assert!(!outside.join("new.txt").exists());
        assert_eq!(
            std::fs::read_to_string(&secret).expect("read secret"),
            "secret needle\n"
        );

        // Edit through the file symlink must not change the target.
        super::edit_file_in_workspace(&at("linkfile.txt"), "secret", "pwned", false, &root)
            .expect_err("edit through file symlink should be refused");
        assert_eq!(
            std::fs::read_to_string(&secret).expect("read secret"),
            "secret needle\n"
        );

        // Search bases through the directory symlink are refused.
        super::glob_search_in_workspace("*.txt", Some(at("linkdir").as_str()), &root)
            .expect_err("glob through dir symlink should be refused");
        super::grep_search_in_workspace(&grep_input("needle", Some(at("linkdir"))), &root)
            .expect_err("grep through dir symlink should be refused");

        // Glob over the workspace drops the file symlink whose target is outside.
        let root_path = root.to_string_lossy().into_owned();
        let glob = super::glob_search_in_workspace("*.txt", Some(&root_path), &root)
            .expect("glob inside workspace should succeed");
        assert!(glob
            .filenames
            .iter()
            .all(|name| !name.ends_with("linkfile.txt")));
        assert!(glob
            .filenames
            .iter()
            .any(|name| name.ends_with("inside.txt")));

        std::fs::remove_dir_all(root.parent().expect("base")).expect("cleanup");
    }

    #[test]
    fn workspace_tools_keep_inside_paths_working() {
        let (root, _outside) = boundary_fixture("inside");
        let at = |rel: &str| root.join(rel).to_string_lossy().into_owned();
        let root_str = root.to_string_lossy().into_owned();

        let read = super::read_file_in_workspace(&at("inside.txt"), None, None, &root)
            .expect("read inside should succeed");
        assert_eq!(read.file.content, "inside needle");

        let absolute_read =
            super::read_file_in_workspace(&format!("{root_str}/inside.txt"), None, None, &root)
                .expect("absolute read inside should succeed");
        assert_eq!(absolute_read.file.total_lines, 1);

        let write = super::write_file_in_workspace(
            &format!("{root_str}/nested/dir/new.txt"),
            "hello\n",
            &root,
        )
        .expect("write inside should succeed");
        assert_eq!(write.kind, "create");
        assert_eq!(
            std::fs::read_to_string(root.join("nested/dir/new.txt")).expect("read new"),
            "hello\n"
        );

        // A `..` that stays inside the workspace is still allowed.
        super::write_file_in_workspace(&at("nested/../renamed.txt"), "moved\n", &root)
            .expect("in-workspace .. should succeed");
        assert!(root.join("renamed.txt").exists());

        let edit =
            super::edit_file_in_workspace(&at("inside.txt"), "inside", "changed", false, &root)
                .expect("edit inside should succeed");
        assert!(!edit.replace_all);

        let glob =
            super::glob_search_in_workspace("*.txt", Some(&root_str), &root).expect("glob inside");
        assert!(glob
            .filenames
            .iter()
            .any(|name| name.ends_with("inside.txt")));

        let grep = super::grep_search_in_workspace(&grep_input("changed", Some(root_str)), &root)
            .expect("grep inside");
        assert_eq!(grep.num_files, 1);

        std::fs::remove_dir_all(root.parent().expect("base")).expect("cleanup");
    }
}
