//! End-to-end Codex request through the real binary.
//!
//! A local server speaks the Responses API's streaming format. The test checks that a
//! Codex model is sent to `/v1/responses` with the expected body, and that the streamed
//! text reaches the output.

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

/// Accepts one request, returns (path, body) and replies with a streamed Responses reply.
fn serve_one_responses_request(
    listener: TcpListener,
) -> thread::JoinHandle<(String, serde_json::Value)> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request");
        let (path, body) = read_request(&mut stream);
        let frames = [
            sse("response.created", r#"{"type":"response.created","response":{"id":"resp_test"}}"#),
            sse("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"message"}}"#),
            sse("response.output_text.delta", r#"{"type":"response.output_text.delta","output_index":0,"delta":"Hello from Codex"}"#),
            sse("response.output_item.done", r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message"}}"#),
            sse("response.completed", r#"{"type":"response.completed","response":{"id":"resp_test","status":"completed","usage":{"input_tokens":5,"output_tokens":3}}}"#),
        ]
        .concat();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{frames}",
            frames.len()
        )
        .expect("write reply");
        (path, body)
    })
}

fn sse(event: &str, data: &str) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

fn read_request(stream: &mut std::net::TcpStream) -> (String, serde_json::Value) {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        assert!(
            read > 0,
            "connection closed before the request was complete"
        );
        data.extend_from_slice(&buffer[..read]);
        let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
        let length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if data.len() >= end + 4 + length {
            let path = headers
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_string();
            let body = serde_json::from_slice(&data[end + 4..end + 4 + length]).expect("json body");
            return (path, body);
        }
    }
}

#[test]
fn codex_model_is_sent_to_the_responses_endpoint_and_streams_text() {
    let root = std::env::temp_dir().join(format!("codex-responses-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let project: PathBuf = root.join("project");
    let config_home = root.join("config");
    fs::create_dir_all(&project).expect("project dir");
    fs::create_dir_all(&config_home).expect("config dir");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = serve_one_responses_request(listener);

    let output = Command::new(env!("CARGO_BIN_EXE_frontal-code"))
        .args([
            "--provider",
            "openai",
            "--model",
            "gpt-5-codex",
            "prompt",
            "say hi",
        ])
        .current_dir(&project)
        .env("FRONTAL_CODE_CONFIG_HOME", &config_home)
        .env("OPENAI_API_KEY", "test-key")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .env_remove("FRONTAL_API_KEY")
        .env_remove("FRONTAL_AUTH_TOKEN")
        .output()
        .expect("run frontal-code");

    let (path, body) = server.join().expect("server");
    let _ = fs::remove_dir_all(&root);

    assert_eq!(path, "/v1/responses");
    assert_eq!(body["model"], "gpt-5-codex");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Hello from Codex"), "{stdout}");
}

#[test]
fn codex_model_without_provider_flag_selects_openai() {
    let root = std::env::temp_dir().join(format!("codex-default-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let project: PathBuf = root.join("project");
    let config_home = root.join("config");
    fs::create_dir_all(&project).expect("project dir");
    fs::create_dir_all(&config_home).expect("config dir");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = serve_one_responses_request(listener);

    // No --provider: the default provider is `frontal`, but a Codex model must still
    // go to OpenAI's Responses endpoint.
    let output = Command::new(env!("CARGO_BIN_EXE_frontal-code"))
        .args(["--model", "gpt-5-codex", "prompt", "say hi"])
        .current_dir(&project)
        .env("FRONTAL_CODE_CONFIG_HOME", &config_home)
        .env("OPENAI_API_KEY", "test-key")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .env_remove("FRONTAL_API_KEY")
        .env_remove("FRONTAL_AUTH_TOKEN")
        .output()
        .expect("run frontal-code");

    let (path, body) = server.join().expect("server");
    let _ = fs::remove_dir_all(&root);

    assert_eq!(path, "/v1/responses");
    assert_eq!(body["model"], "gpt-5-codex");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
