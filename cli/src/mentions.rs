//! `@path` mentions: completing workspace file paths and attaching them to a prompt.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Most files attached to one prompt.
pub const MAX_ATTACHED_FILES: usize = 5;
/// Largest file (in bytes) that is attached in full.
pub const MAX_ATTACHED_BYTES: u64 = 64 * 1024;
/// Upper bound on completion candidates, so huge repositories stay responsive.
const MAX_CANDIDATES: usize = 5_000;

/// Workspace-relative paths of tracked and untracked files, from git. Empty outside a repo.
pub fn workspace_files(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .take(MAX_CANDIDATES)
        .map(str::to_string)
        .collect()
}

/// Expands `@path` tokens in `input`. Each readable text file inside `root` is appended as a
/// fenced block. Returns the prompt and the paths that were attached. Mentions that do not
/// resolve to an allowed file are left in the text unchanged.
pub fn expand(input: &str, root: &Path) -> (String, Vec<String>) {
    let mut attached: Vec<(String, String)> = Vec::new();
    for token in input.split_whitespace() {
        if attached.len() >= MAX_ATTACHED_FILES {
            break;
        }
        let Some(relative) = token.strip_prefix('@') else {
            continue;
        };
        if attached.iter().any(|(path, _)| path == relative) {
            continue;
        }
        if let Some(content) = read_allowed_file(root, relative) {
            attached.push((relative.to_string(), content));
        }
    }

    if attached.is_empty() {
        return (input.to_string(), Vec::new());
    }

    let mut prompt = input.to_string();
    let mut names = Vec::new();
    for (path, content) in attached {
        let _ = write!(prompt, "\n\nFile: {path}\n```\n{content}\n```");
        names.push(path);
    }
    (prompt, names)
}

/// Reads `relative` only when it is a regular file inside `root`, within the size limit,
/// and is valid UTF-8. Paths that escape the workspace (`..`, absolute, symlinks) are refused.
fn read_allowed_file(root: &Path, relative: &str) -> Option<String> {
    if relative.is_empty() || Path::new(relative).is_absolute() {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let candidate: PathBuf = root.join(relative).canonicalize().ok()?;
    if !candidate.starts_with(&root) {
        return None;
    }
    let metadata = fs::metadata(&candidate).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ATTACHED_BYTES {
        return None;
    }
    fs::read_to_string(&candidate).ok()
}

#[cfg(test)]
mod tests {
    use super::{expand, MAX_ATTACHED_BYTES, MAX_ATTACHED_FILES};
    use std::fmt::Write as _;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mentions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).expect("root should create");
        fs::write(root.join("src/lib.rs"), "fn main() {}\n").expect("write");
        root
    }

    #[test]
    fn attaches_workspace_files_mentioned_with_at_sign() {
        let root = temp_root("attach");
        let (prompt, names) = expand("explain @src/lib.rs please", &root);

        assert_eq!(names, vec!["src/lib.rs".to_string()]);
        assert!(prompt.starts_with("explain @src/lib.rs please"));
        assert!(prompt.contains("File: src/lib.rs"));
        assert!(prompt.contains("fn main() {}"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn leaves_unknown_and_escaping_mentions_untouched() {
        let root = temp_root("escape");
        for input in ["@missing.rs", "@../outside", "@/etc/hosts", "@src"] {
            let (prompt, names) = expand(input, &root);
            assert_eq!(prompt, input);
            assert!(names.is_empty(), "{input} should not attach");
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn limits_file_count_and_size() {
        let root = temp_root("limits");
        for index in 0..(MAX_ATTACHED_FILES + 2) {
            fs::write(root.join(format!("f{index}.txt")), "x").expect("write");
        }
        fs::write(
            root.join("big.txt"),
            vec![b'a'; usize::try_from(MAX_ATTACHED_BYTES + 1).expect("fits in usize")],
        )
        .expect("write");

        let mut mentions = String::new();
        for index in 0..(MAX_ATTACHED_FILES + 2) {
            let _ = write!(mentions, "@f{index}.txt ");
        }
        mentions.push_str("@big.txt");
        let (_, names) = expand(&mentions, &root);

        assert_eq!(names.len(), MAX_ATTACHED_FILES);
        assert!(!names.iter().any(|name| name == "big.txt"));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
