//! End-to-end checks of the `PostHog` transport against a local stub server.
//! No external network is used.

#![cfg(feature = "posthog")]

use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use code_analytics::{
    AnalyticsBuilder, AnalyticsClient, Environment, Event, ExitKind, Outcome, SessionMode, ToolKind,
};

/// Records every request body and can fail the first N requests with a 500.
struct StubServer {
    url: String,
    bodies: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl StubServer {
    fn start(fail_first: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("stub should bind");
        listener
            .set_nonblocking(true)
            .expect("stub should be non-blocking");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));

        let bodies = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let remaining_failures = Arc::new(AtomicUsize::new(fail_first));

        let thread_bodies = Arc::clone(&bodies);
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let fail = remaining_failures
                            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                                Some(n.saturating_sub(1))
                            })
                            .is_ok_and(|previous| previous > 0);
                        handle_connection(stream, &thread_bodies, fail);
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            url,
            bodies,
            stop,
            handle: Some(handle),
        }
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().expect("bodies lock").clone()
    }

    fn wait_for_bodies(&self, at_least: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let bodies = self.bodies();
            if bodies.len() >= at_least || Instant::now() >= deadline {
                return bodies;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(mut stream: std::net::TcpStream, bodies: &Mutex<Vec<String>>, fail: bool) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    while buffer.len() < header_end + content_length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }

    let body_end = (header_end + content_length).min(buffer.len());
    let body = String::from_utf8_lossy(&buffer[header_end..body_end]).to_string();
    bodies.lock().expect("bodies lock").push(body);

    let response = if fail {
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_string()
    } else {
        let payload = r#"{"status":1}"#;
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    };
    let _ = stream.write_all(response.as_bytes());
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn enabled_builder(host: &str) -> AnalyticsBuilder {
    AnalyticsBuilder::default()
        .api_key(Some("phc_test_key".to_string()))
        .host(host)
        .settings_enabled(Some(true))
        .app_version("9.9.9")
        .environment(Environment::Development)
        .flush_interval(Duration::from_millis(50))
}

#[test]
fn delivers_typed_events_with_only_allowed_properties() {
    let stub = StubServer::start(0);
    let client = enabled_builder(&stub.url).build();
    assert!(client.is_enabled(), "client should be enabled with opt-in");

    client.capture(&Event::SessionStarted {
        mode: SessionMode::Repl,
    });
    client.capture(&Event::ToolUsed {
        tool: ToolKind::Bash,
        outcome: Outcome::Success,
        duration_ms: 12,
    });
    client.capture(&Event::SessionEnded {
        duration_secs: 3,
        turns: 1,
        exit: ExitKind::Ok,
    });
    client.shutdown();

    let bodies = stub.wait_for_bodies(1);
    let all = bodies.join("\n");
    assert!(all.contains("session_started"), "missing event name: {all}");
    assert!(all.contains("\"mode\":\"repl\""), "missing mode: {all}");
    assert!(all.contains("tool_used"), "missing tool event: {all}");
    assert!(
        all.contains("\"tool\":\"bash\""),
        "missing tool label: {all}"
    );
    assert!(
        all.contains("\"app_version\":\"9.9.9\""),
        "missing version: {all}"
    );
    assert!(
        all.contains("\"environment\":\"development\""),
        "missing environment: {all}"
    );
    assert!(
        all.contains("\"$process_person_profile\":false"),
        "person profiles must be disabled: {all}"
    );
    assert!(!all.contains("/Users/"), "a path leaked: {all}");
    assert!(!all.contains("prompt"), "prompt-like key leaked: {all}");
}

#[test]
fn opted_out_client_sends_nothing() {
    let stub = StubServer::start(0);
    let client = AnalyticsBuilder::default()
        .api_key(Some("phc_test_key".to_string()))
        .host(&stub.url)
        .settings_enabled(Some(true))
        .env_disabled(true)
        .build();
    assert!(!client.is_enabled());

    client.capture(&Event::SessionStarted {
        mode: SessionMode::Prompt,
    });
    client.shutdown();
    thread::sleep(Duration::from_millis(300));

    assert!(stub.bodies().is_empty(), "opted-out client made a request");
}

#[test]
fn retries_after_a_server_error() {
    let stub = StubServer::start(1);
    let client = enabled_builder(&stub.url).build();

    client.capture(&Event::ErrorOccurred {
        class: code_analytics::ErrorClass::Provider,
    });
    // Shut down straight away: the sink must wait out the retry before exiting.
    client.shutdown();
    let bodies = stub.bodies();
    assert!(
        bodies.len() >= 2,
        "expected a retry after the 500, saw {} request(s)",
        bodies.len()
    );
    assert!(bodies.iter().all(|body| body.contains("error_occurred")));
}

#[test]
fn capture_never_blocks_and_shutdown_is_bounded_when_host_is_down() {
    // Bind then drop to get a port that refuses connections.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("probe should bind")
        .local_addr()
        .expect("local addr")
        .port();
    let client = enabled_builder(&format!("http://127.0.0.1:{port}")).build();

    let capture_started = Instant::now();
    for _ in 0..200 {
        client.capture(&Event::SessionStarted {
            mode: SessionMode::Repl,
        });
    }
    assert!(
        capture_started.elapsed() < Duration::from_millis(500),
        "capture blocked for {:?}",
        capture_started.elapsed()
    );

    let shutdown_started = Instant::now();
    client.shutdown();
    assert!(
        shutdown_started.elapsed() < Duration::from_secs(6),
        "shutdown took {:?} with the host down",
        shutdown_started.elapsed()
    );
}

#[test]
fn shutdown_twice_and_capture_after_are_safe() {
    let stub = StubServer::start(0);
    let client: AnalyticsClient = enabled_builder(&stub.url).build();
    client.shutdown();
    client.shutdown();
    client.capture(&Event::SessionStarted {
        mode: SessionMode::Repl,
    });
    client.flush();
}
