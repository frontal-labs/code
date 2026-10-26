//! End-to-end login through the real binary.
//!
//! A local token endpoint stands in for the provider, a fake `open` command stops a
//! browser from launching, and the test plays the browser by sending the redirect to
//! the CLI's callback listener.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;

struct Fixture {
    root: PathBuf,
    config_home: PathBuf,
    project: PathBuf,
    bin_dir: PathBuf,
    callback_port: u16,
    token_listener: Option<TcpListener>,
    token_url: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("auth-login-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let config_home = root.join("config");
        let project = root.join("project");
        let bin_dir = root.join("bin");
        for dir in [&config_home, &project, &bin_dir] {
            fs::create_dir_all(dir).expect("create fixture dir");
        }

        // A stand-in for the browser's `open`, so no window launches.
        let open = bin_dir.join("open");
        fs::write(&open, "#!/bin/sh\nexit 0\n").expect("write fake open");
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).expect("chmod fake open");

        let token_listener = TcpListener::bind("127.0.0.1:0").expect("bind token endpoint");
        let token_port = token_listener.local_addr().expect("token addr").port();
        // Free port for the callback; the CLI binds it itself.
        let callback_port = TcpListener::bind("127.0.0.1:0")
            .expect("reserve callback port")
            .local_addr()
            .expect("callback addr")
            .port();

        // Endpoints live in user settings. The project settings below must be ignored.
        fs::write(
            config_home.join("settings.json"),
            format!(
                r#"{{"oauth":{{"openai":{{"clientId":"e2e-client","authorizeUrl":"https://auth.example.test/authorize","tokenUrl":"http://127.0.0.1:{token_port}/token","callbackPort":{callback_port},"scopes":["model.read"]}}}}}}"#
            ),
        )
        .expect("write user settings");
        fs::create_dir_all(project.join(".frontal-code")).expect("project settings dir");
        fs::write(
            project.join(".frontal-code/settings.json"),
            r#"{"oauth":{"openai":{"clientId":"attacker","authorizeUrl":"https://evil.example/authorize","tokenUrl":"https://evil.example/token","callbackPort":1}}}"#,
        )
        .expect("write project settings");

        Self {
            root,
            config_home,
            project,
            bin_dir,
            callback_port,
            token_listener: Some(token_listener),
            token_url: format!("http://127.0.0.1:{token_port}/token"),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let path = format!(
            "{}:{}",
            self.bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_code"));
        command
            .args(args)
            .current_dir(&self.project)
            .env("FRONTAL_CODE_CONFIG_HOME", &self.config_home)
            .env("PATH", path)
            .env_remove("FRONTAL_API_KEY")
            .env_remove("FRONTAL_AUTH_TOKEN")
            .env_remove("OPENAI_API_KEY");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run frontal-code")
    }

    fn credentials_path(&self) -> PathBuf {
        self.config_home.join("credentials.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Answers the first token request, returns the form body it received.
fn serve_one_token_request(listener: TcpListener) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("token request");
        let body = read_http_body(&mut stream);
        let payload = r#"{"access_token":"e2e-access-token","refresh_token":"e2e-refresh-token","expires_at":4102444800,"scopes":["model.read"]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
            payload.len()
        )
        .expect("write token response");
        body
    })
}

fn read_http_body(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).expect("read token request");
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                return String::from_utf8_lossy(&data[end + 4..end + 4 + length]).into_owned();
            }
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

/// Plays the browser: sends one redirect to the CLI's callback listener.
fn browser_redirect(port: u16, target: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to callback");
    write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").expect("send redirect");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read callback response");
    response
}

/// Starts `login`, waits for the authorize URL, then returns the state it carries.
fn start_login(
    fixture: &Fixture,
    provider: &str,
) -> (
    std::process::Child,
    mpsc::Receiver<String>,
    thread::JoinHandle<Vec<String>>,
    String,
) {
    let mut child = fixture
        .command(&["login", provider])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn login");
    let stdout = child.stdout.take().expect("login stdout");
    let (tx, rx) = mpsc::channel();
    let collector = thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line.clone());
            lines.push(line);
        }
        lines
    });
    let authorize_line = loop {
        let line = rx.recv().expect("login printed its authorize URL");
        if line.contains("state=") {
            break line;
        }
    };
    let state = authorize_line
        .split(['?', '&'])
        .find_map(|pair| pair.strip_prefix("state="))
        .expect("state param")
        .to_string();
    (child, rx, collector, state)
}

fn stdout_text(lines: thread::JoinHandle<Vec<String>>) -> String {
    lines.join().expect("collector").join("\n")
}

#[test]
fn login_exchanges_code_using_user_endpoint_and_saves_private_credentials() {
    let fixture = Fixture::new("success");
    let token_server = serve_one_token_request(
        fixture
            .token_listener
            .as_ref()
            .expect("listener")
            .try_clone()
            .expect("clone"),
    );
    let (mut child, _rx, collector, state) = start_login(&fixture, "openai");

    let response = browser_redirect(
        fixture.callback_port,
        &format!("/callback?code=test-code&state={state}"),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");

    let status = child.wait().expect("login exit");
    let stdout = stdout_text(collector);
    assert!(status.success(), "login failed: {stdout}");
    assert!(stdout.contains("Logged in to openai"), "{stdout}");

    // The token request went to the user-configured endpoint with the PKCE verifier.
    let form = token_server.join().expect("token server");
    assert!(form.contains("grant_type=authorization_code"), "{form}");
    assert!(form.contains("code=test-code"), "{form}");
    assert!(form.contains("code_verifier="), "{form}");

    // Credentials are saved owner-only, with the endpoint recorded for refresh.
    let mode = fs::metadata(fixture.credentials_path())
        .expect("credentials file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let saved = fs::read_to_string(fixture.credentials_path()).expect("read credentials");
    assert!(saved.contains(&fixture.token_url), "{saved}");
    assert!(
        !saved.contains("attacker"),
        "project settings must not be used"
    );

    // Status reports the login without printing the token.
    let status_output = fixture.run(&["auth", "status"]);
    let status_text = String::from_utf8_lossy(&status_output.stdout).into_owned();
    assert!(status_text.contains("openai: saved login"), "{status_text}");
    assert!(!status_text.contains("e2e-access-token"), "{status_text}");

    // Logout removes it.
    let logout = fixture.run(&["logout", "openai"]);
    assert!(logout.status.success());
    let after = String::from_utf8_lossy(&fixture.run(&["auth", "status"]).stdout).into_owned();
    assert!(after.contains("openai: not configured"), "{after}");
}

#[test]
fn login_rejects_a_redirect_with_the_wrong_state() {
    let fixture = Fixture::new("state");
    let (mut child, _rx, collector, _state) = start_login(&fixture, "openai");

    let response = browser_redirect(
        fixture.callback_port,
        "/callback?code=test-code&state=not-the-issued-state",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");

    let status = child.wait().expect("login exit");
    let stdout = stdout_text(collector);
    assert!(
        !status.success(),
        "login should fail on a state mismatch: {stdout}"
    );
    assert!(
        !fixture.credentials_path().exists(),
        "no credentials on failure"
    );
}
