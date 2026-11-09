//! Server-side publication of a worker's changes.
//!
//! A worker never pushes. It sends its changes as a patch, and the server applies that
//! patch to a fresh clone of the repository and pushes the result. Only the server holds
//! the GitHub token, so a compromised worker cannot push, and the checkout's `.git`
//! directory, which the worker can write to, is never trusted.
//!
//! Git runs with hooks disabled, with no system or user config, and with no terminal
//! prompts. The GitHub token reaches git only through the environment, never through
//! argv, a URL, or a file on disk.

use std::path::Path;
use std::process::Command;

/// Largest patch the server accepts.
pub(crate) const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;

/// Branches the worker may publish to. Anything else could target a protected branch.
pub(crate) const PUBLISH_BRANCH_PREFIX: &str = "frontal-code/";

/// Paths a worker may not change. `.github/workflows` changes CI behavior, and `.git`
/// holds repository configuration.
const FORBIDDEN_PATH_PREFIXES: &[&str] = &[".git/", ".github/workflows/", ".github/actions/"];

/// Checks a patch before it is applied. The checks are about what the patch does to the
/// repository, not about its content: it must not touch `.git`, CI definitions, or
/// symbolic links and submodules, which can point outside the tree.
pub(crate) fn validate_patch(patch: &str) -> Result<(), String> {
    if patch.len() > MAX_PATCH_BYTES {
        return Err(format!("patch is larger than {MAX_PATCH_BYTES} bytes"));
    }
    if patch.trim().is_empty() {
        return Err("patch is empty".to_string());
    }
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            // "diff --git a/<old> b/<new>"; the names can contain spaces, so check both
            // halves of the header rather than splitting on the first space.
            let paths = rest
                .split_once(" b/")
                .map(|(old, new)| vec![old, new])
                .ok_or_else(|| "malformed diff header".to_string())?;
            for path in paths {
                check_path(path)?;
            }
        } else if let Some(path) = line
            .strip_prefix("rename from ")
            .or_else(|| line.strip_prefix("rename to "))
            .or_else(|| line.strip_prefix("copy from "))
            .or_else(|| line.strip_prefix("copy to "))
        {
            check_path(path)?;
        } else if line.starts_with("new file mode 120000")
            || line.starts_with("new mode 120000")
            || line.contains("mode 120000")
        {
            return Err("patch creates a symbolic link, which is not allowed".to_string());
        } else if line.starts_with("new file mode 160000") || line.contains("mode 160000") {
            return Err("patch adds a submodule, which is not allowed".to_string());
        }
    }
    Ok(())
}

fn check_path(path: &str) -> Result<(), String> {
    let path = path.trim_matches('"');
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Err(format!("patch path is not a relative path: {path}"));
    }
    if path
        .split('/')
        .any(|segment| segment == ".." || segment.is_empty())
    {
        return Err(format!("patch path escapes the repository: {path}"));
    }
    let dotted = format!("{path}/");
    for forbidden in FORBIDDEN_PATH_PREFIXES {
        if dotted.starts_with(forbidden) || path == forbidden.trim_end_matches('/') {
            return Err(format!("patch changes a protected path: {path}"));
        }
    }
    Ok(())
}

/// Checks that a branch name is one the worker may publish to.
pub(crate) fn validate_branch(branch: &str) -> Result<(), String> {
    if !branch.starts_with(PUBLISH_BRANCH_PREFIX) {
        return Err(format!(
            "tasks may only publish to branches starting with `{PUBLISH_BRANCH_PREFIX}`"
        ));
    }
    if branch.contains("..")
        || branch.contains(' ')
        || branch.ends_with('/')
        || branch.to_ascii_lowercase().ends_with(".lock")
    {
        return Err(format!("invalid branch name: {branch}"));
    }
    Ok(())
}

/// Everything the server needs to publish one task's changes.
pub(crate) struct PublishRequest<'a> {
    pub task_id: &'a str,
    pub prompt: &'a str,
    pub repo_url: &'a str,
    pub base_ref: Option<&'a str>,
    pub branch: &'a str,
    pub patch: &'a str,
    pub workspace_root: &'a Path,
    pub github_token: Option<&'a str>,
    pub author_name: &'a str,
    pub author_email: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishOutcome {
    pub branch: String,
    pub commit_sha: String,
    pub remote: String,
}

/// Applies `patch` to a fresh clone of `repo_url` on `branch`, commits it, and pushes.
/// The clone is removed afterwards, whether or not the push succeeded.
pub(crate) fn publish_changes(request: &PublishRequest<'_>) -> Result<PublishOutcome, String> {
    // A repository URL that starts with `-` would be read by git as an option.
    if request.repo_url.starts_with('-') {
        return Err("repository URL must not start with `-`".to_string());
    }
    validate_patch(request.patch)?;
    validate_branch(request.branch)?;

    // The scratch name is random and carries no request data, so no path is built from input.
    let nonce = random_hex(6)?;
    let scratch = request.workspace_root.join("publish").join(nonce);
    std::fs::create_dir_all(&scratch).map_err(|error| format!("publish scratch: {error}"))?;
    let result = publish_in(&scratch, request);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn publish_in(scratch: &Path, request: &PublishRequest<'_>) -> Result<PublishOutcome, String> {
    let repo = scratch.join("repo");
    let home = scratch.join("home");
    std::fs::create_dir_all(&home).map_err(|error| format!("publish scratch: {error}"))?;
    let patch_path = scratch.join("changes.patch");
    std::fs::write(&patch_path, request.patch)
        .map_err(|error| format!("publish scratch: {error}"))?;

    let auth = AuthEnv::for_repo(request.repo_url, request.github_token);
    run_git(
        scratch,
        &home,
        &auth,
        &[
            "clone",
            "--no-checkout",
            "--quiet",
            "--",
            request.repo_url,
            repo.to_str().ok_or("scratch path is not UTF-8")?,
        ],
        request.github_token,
    )?;
    let base = request.base_ref.unwrap_or("HEAD");
    run_git(
        &repo,
        &home,
        &auth,
        &["checkout", "--quiet", base],
        request.github_token,
    )?;
    run_git(
        &repo,
        &home,
        &auth,
        &["checkout", "--quiet", "-b", request.branch],
        request.github_token,
    )?;
    run_git(
        &repo,
        &home,
        &auth,
        &[
            "apply",
            "--index",
            "--whitespace=nowarn",
            patch_path.to_str().ok_or("patch path")?,
        ],
        request.github_token,
    )
    .map_err(|error| format!("the worker's changes do not apply to {base}: {error}"))?;

    let message = commit_message(request.task_id, request.prompt);
    run_git(
        &repo,
        &home,
        &auth,
        &[
            "-c",
            &format!("user.name={}", request.author_name),
            "-c",
            &format!("user.email={}", request.author_email),
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            &message,
        ],
        request.github_token,
    )?;
    let commit_sha = run_git(
        &repo,
        &home,
        &auth,
        &["rev-parse", "HEAD"],
        request.github_token,
    )?
    .trim()
    .to_string();
    run_git(
        &repo,
        &home,
        &auth,
        &[
            "push",
            "--quiet",
            "origin",
            &format!("HEAD:refs/heads/{}", request.branch),
        ],
        request.github_token,
    )?;
    Ok(PublishOutcome {
        branch: request.branch.to_string(),
        commit_sha,
        remote: "origin".to_string(),
    })
}

/// Authentication for git over HTTPS to GitHub, passed through `GIT_CONFIG_*` in the
/// environment so it never appears in argv or in `.git/config`.
struct AuthEnv {
    vars: Vec<(String, String)>,
}

impl AuthEnv {
    fn for_repo(repo_url: &str, token: Option<&str>) -> Self {
        let Some(token) = token else {
            return Self { vars: Vec::new() };
        };
        if !repo_url.starts_with("https://github.com/") {
            return Self { vars: Vec::new() };
        }
        let credentials = base64_encode(format!("x-access-token:{token}").as_bytes());
        Self {
            vars: vec![
                ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
                (
                    "GIT_CONFIG_KEY_0".to_string(),
                    "http.https://github.com/.extraHeader".to_string(),
                ),
                (
                    "GIT_CONFIG_VALUE_0".to_string(),
                    format!("AUTHORIZATION: basic {credentials}"),
                ),
            ],
        }
    }
}

/// Runs git with hooks disabled and no user or system configuration. Errors never
/// include the token: git's output is scrubbed of it first.
fn run_git(
    cwd: &Path,
    home: &Path,
    auth: &AuthEnv,
    args: &[&str],
    token: Option<&str>,
) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("protocol.file.allow=always");
    for (key, value) in &auth.vars {
        command.env(key, value);
    }
    command.args(args);

    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if let Some(token) = token {
        stderr = stderr.replace(token, "***");
    }
    Err(format!(
        "git {} failed: {}",
        args.first().copied().unwrap_or(""),
        stderr.trim()
    ))
}

fn commit_message(task_id: &str, prompt: &str) -> String {
    let summary: String = prompt
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(72)
        .collect();
    if summary.is_empty() {
        format!("Hosted task {task_id}")
    } else {
        format!("Hosted task {task_id}: {summary}")
    }
}

/// Refuses a GitHub repository the token may not write to. The allowlist is the only
/// way the token is used: an empty allowlist means no GitHub publication at all. Local
/// repositories never use the token, so they are not checked.
pub(crate) fn check_github_publication_allowed(
    repo_url: &str,
    token: Option<&str>,
    allowed_repos: &[String],
) -> Result<(), String> {
    let Some((owner, repo)) = github_owner_repo(repo_url) else {
        return Ok(());
    };
    if token.is_none() {
        return Ok(());
    }
    if allowed_repos.is_empty() {
        return Err(
            "GITHUB_TOKEN is set but FRONTAL_CODE_GITHUB_ALLOWED_REPOS is empty; name the \
             repositories the token may write to (for example acme/widgets)"
                .to_string(),
        );
    }
    let target = format!("{owner}/{repo}");
    let allowed = allowed_repos.iter().any(|entry| {
        entry == &target
            || entry
                .strip_suffix("/*")
                .is_some_and(|prefix| prefix == owner)
    });
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "{target} is not in FRONTAL_CODE_GITHUB_ALLOWED_REPOS"
        ))
    }
}

/// Parses `FRONTAL_CODE_GITHUB_ALLOWED_REPOS`: comma-separated `owner/repo` or `owner/*`.
pub(crate) fn parse_allowed_repos(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// What to open a pull request for. `base` is the target branch; when it is `None`, the
/// repository's default branch is used.
pub(crate) struct PullRequest<'a> {
    pub api_base: &'a str,
    pub token: &'a str,
    pub repo_url: &'a str,
    pub head: &'a str,
    pub base: Option<&'a str>,
    pub title: &'a str,
    pub body: &'a str,
}

/// Opens a pull request for a published branch through the GitHub REST API.
pub(crate) async fn open_pull_request(
    client: &reqwest::Client,
    request: &PullRequest<'_>,
) -> Result<serde_json::Value, String> {
    let PullRequest {
        api_base,
        token,
        repo_url,
        head,
        base,
        title,
        body,
    } = *request;
    let (owner, repo) = github_owner_repo(repo_url)
        .ok_or_else(|| "pull requests are only opened for github.com repositories".to_string())?;
    let api = api_base.trim_end_matches('/');
    let base = if let Some(base) = base {
        base.to_string()
    } else {
        let info: serde_json::Value = github_request(
            client,
            reqwest::Method::GET,
            token,
            &format!("{api}/repos/{owner}/{repo}"),
        )
        .send()
        .await
        .map_err(|error| format!("GitHub repository lookup failed: {}", error.without_url()))?
        .json()
        .await
        .map_err(|error| format!("GitHub repository lookup failed: {}", error.without_url()))?;
        info.get("default_branch")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("main")
            .to_string()
    };
    let response = github_request(
        client,
        reqwest::Method::POST,
        token,
        &format!("{api}/repos/{owner}/{repo}/pulls"),
    )
    .json(&serde_json::json!({
        "title": title,
        "head": head,
        "base": base,
        "body": body,
    }))
    .send()
    .await
    .map_err(|error| format!("GitHub pull request failed: {}", error.without_url()))?;
    let status = response.status();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("GitHub pull request failed: {}", error.without_url()))?;
    if status.is_success() {
        Ok(value)
    } else {
        let message = value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("request refused");
        Err(format!(
            "GitHub refused the pull request ({status}): {message}"
        ))
    }
}

fn github_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    token: &str,
    url: &str,
) -> reqwest::RequestBuilder {
    // The token is sent to the API only. Its value never reaches an error message: errors
    // above use `without_url`, and GitHub's message field carries no credentials.
    client
        .request(method, url)
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .header("user-agent", "frontal-code")
}

/// Parses `https://github.com/<owner>/<repo>(.git)` into its parts.
pub(crate) fn github_owner_repo(repo_url: &str) -> Option<(String, String)> {
    let path = repo_url.strip_prefix("https://github.com/")?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let (owner, repo) = path.split_once('/')?;
    (!owner.is_empty() && !repo.is_empty() && !repo.contains('/'))
        .then(|| (owner.to_string(), repo.to_string()))
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0_u8; bytes];
    getrandom::getrandom(&mut buffer).map_err(|error| format!("random source: {error}"))?;
    Ok(hex::encode(buffer))
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE_PATCH: &str = "diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
";

    #[test]
    fn a_repository_url_that_looks_like_an_option_is_refused_before_git_runs() {
        let workspace = std::env::temp_dir().join("frontal-code-publish-option-guard");
        let request = PublishRequest {
            workspace_root: &workspace,
            task_id: "task-1",
            prompt: "",
            repo_url: "--upload-pack=touch /tmp/pwned",
            base_ref: None,
            branch: "frontal-code/guard",
            patch: "",
            github_token: None,
            author_name: "t",
            author_email: "t@example.com",
        };
        let error = publish_changes(&request).expect_err("option-like URL must be refused");
        assert!(error.contains("must not start with `-`"), "{error}");
        assert!(
            !workspace.join("publish").exists(),
            "no scratch directory before the check"
        );
    }

    #[test]
    fn ordinary_changes_pass_validation() {
        assert!(validate_patch(SAFE_PATCH).is_ok());
    }

    #[test]
    fn changes_to_git_ci_or_escaping_paths_are_refused() {
        for path in [
            ".git/config",
            ".github/workflows/ci.yml",
            "../outside.txt",
            "/etc/passwd",
            "a/../../b",
        ] {
            let patch = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n");
            assert!(validate_patch(&patch).is_err(), "should refuse {path}");
        }
    }

    #[test]
    fn symlinks_and_submodules_are_refused() {
        let link = "diff --git a/link b/link\nnew file mode 120000\n--- /dev/null\n+++ b/link\n";
        assert!(validate_patch(link).unwrap_err().contains("symbolic link"));
        let module = "diff --git a/sub b/sub\nnew file mode 160000\n";
        assert!(validate_patch(module).unwrap_err().contains("submodule"));
    }

    #[test]
    fn publish_branches_are_namespaced() {
        assert!(validate_branch("frontal-code/task-1").is_ok());
        assert!(validate_branch("main").is_err());
        assert!(validate_branch("frontal-code/../main").is_err());
    }

    #[test]
    fn oversized_and_empty_patches_are_refused() {
        assert!(validate_patch("").is_err());
        assert!(validate_patch(&"x".repeat(MAX_PATCH_BYTES + 1)).is_err());
    }

    #[test]
    fn publication_needs_an_explicit_repository_allowlist_when_a_token_is_set() {
        let repo = "https://github.com/acme/widgets.git";
        assert!(check_github_publication_allowed(repo, None, &[]).is_ok());
        assert!(check_github_publication_allowed(repo, Some("t"), &[]).is_err());
        assert!(
            check_github_publication_allowed(repo, Some("t"), &["acme/widgets".to_string()])
                .is_ok()
        );
        assert!(check_github_publication_allowed(repo, Some("t"), &["acme/*".to_string()]).is_ok());
        assert!(
            check_github_publication_allowed(repo, Some("t"), &["acme/other".to_string()]).is_err()
        );
        assert!(
            check_github_publication_allowed(repo, Some("t"), &["evil/*".to_string()]).is_err()
        );
        // Local repositories never see the token, so the allowlist does not apply.
        assert!(check_github_publication_allowed("/local/repo", Some("t"), &[]).is_ok());
        assert_eq!(
            parse_allowed_repos(" acme/widgets, acme/* ,"),
            vec!["acme/widgets", "acme/*"]
        );
    }

    /// A one-shot HTTP server that records the request head and answers with `status` and
    /// `body`.
    fn github_stub(status: u16, body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("addr");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut head = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                head.extend_from_slice(&buffer[..read]);
                if head.windows(4).any(|window| window == b"\r\n\r\n") || read == 0 {
                    break;
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&head).into_owned());
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).expect("write");
        });
        (format!("http://{address}"), receiver)
    }

    #[tokio::test]
    async fn pull_request_is_opened_through_the_rest_api_with_the_token_in_a_header_only() {
        let (base, received) = github_stub(
            201,
            r#"{"number":7,"html_url":"https://github.com/acme/widgets/pull/7"}"#,
        );
        let value = open_pull_request(
            &reqwest::Client::new(),
            &PullRequest {
                api_base: &base,
                token: "ghp_secret",
                repo_url: "https://github.com/acme/widgets.git",
                head: "frontal-code/task-1",
                base: Some("main"),
                title: "Hosted task task-1",
                body: "body",
            },
        )
        .await
        .expect("PR opened");
        assert_eq!(value["number"], 7);
        let head = received.recv().expect("request seen");
        assert!(
            head.starts_with("POST /repos/acme/widgets/pulls "),
            "{head}"
        );
        assert!(head
            .to_ascii_lowercase()
            .contains("authorization: bearer ghp_secret"));
    }

    #[tokio::test]
    async fn pull_request_refusal_is_reported_with_github_message() {
        let (base, _received) = github_stub(422, r#"{"message":"Validation Failed"}"#);
        let error = open_pull_request(
            &reqwest::Client::new(),
            &PullRequest {
                api_base: &base,
                token: "t",
                repo_url: "https://github.com/acme/widgets",
                head: "frontal-code/x",
                base: Some("main"),
                title: "t",
                body: "b",
            },
        )
        .await
        .expect_err("refused");
        assert!(error.contains("Validation Failed"), "{error}");
    }

    #[test]
    fn github_urls_parse_to_owner_and_repo() {
        assert_eq!(
            github_owner_repo("https://github.com/acme/widgets.git"),
            Some(("acme".to_string(), "widgets".to_string()))
        );
        assert_eq!(github_owner_repo("/local/path"), None);
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(
            base64_encode(b"x-access-token:abc"),
            "eC1hY2Nlc3MtdG9rZW46YWJj"
        );
        assert_eq!(base64_encode(b"a"), "YQ==");
    }

    #[test]
    fn publish_applies_the_patch_to_a_fresh_clone_and_pushes_the_branch() {
        let root = std::env::temp_dir().join(format!("publish-test-{}", random_hex(4).unwrap()));
        let remote = root.join("remote.git");
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        git_test(
            &root,
            &["init", "--bare", "--quiet", remote.to_str().unwrap()],
        );
        git_test(&source, &["init", "--quiet", "-b", "main"]);
        std::fs::write(source.join("README.md"), "hello\n").unwrap();
        git_test(&source, &["add", "."]);
        git_test(
            &source,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "--quiet",
                "-m",
                "init",
            ],
        );
        git_test(
            &source,
            &["push", "--quiet", remote.to_str().unwrap(), "main"],
        );

        let patch = "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-hello\n+hello from the worker\n";
        let outcome = publish_changes(&PublishRequest {
            task_id: "task-publish",
            prompt: "Update the readme",
            repo_url: remote.to_str().unwrap(),
            base_ref: Some("main"),
            branch: "frontal-code/task-publish",
            patch,
            workspace_root: &root.join("workspaces"),
            github_token: None,
            author_name: "Frontal Code Worker",
            author_email: "worker@frontal.invalid",
        })
        .expect("publish should succeed");

        assert_eq!(outcome.branch, "frontal-code/task-publish");
        let pushed = git_capture(
            &remote,
            &["rev-parse", "refs/heads/frontal-code/task-publish"],
        );
        assert_eq!(pushed.trim(), outcome.commit_sha);
        let content = git_capture(
            &remote,
            &["show", "refs/heads/frontal-code/task-publish:README.md"],
        );
        assert_eq!(content, "hello from the worker\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn git_test(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn git_capture(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("git");
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}
