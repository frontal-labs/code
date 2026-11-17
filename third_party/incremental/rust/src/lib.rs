//! Rust client for incremental Protocol v1 (docs/spec/PROTOCOL.md, spec/adapters/rust.md).
//!
//! The client spawns `incremental serve`, sends one JSON object per line and
//! matches each answer by id. It never plans: every answer comes from the engine.
//! Methods take `&mut self`, so requests run one at a time in call order.

use serde_json::{json, Map, Value};
use std::env;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PROTOCOL: u64 = 1;
const CLIENT_NAME: &str = "incremental-rust";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const STDERR_LINES: usize = 20;
// Methods that do not change the store. Only these are retried after a restart.
const READ_ONLY: [&str; 6] = ["hello", "baseline", "affected", "explain", "stats", "ping"];

/// A protocol error (`code`, `message`, `data`), or a client-side error such as `timeout`.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl Error {
    fn new(code: &str, message: impl Into<String>, data: Option<Value>) -> Self {
        Error {
            code: code.to_string(),
            message: message.into(),
            data,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Engine command, store, root and timing. `Config::default()` reads
/// `INCREMENTAL_ENGINE` and `INCREMENTAL_STORE`, and uses the enclosing git repository.
#[derive(Debug, Clone)]
pub struct Config {
    /// Command prefix. The client runs `<engine...> serve --store <store> --root <root>`.
    pub engine: Vec<String>,
    pub store: PathBuf,
    pub root: PathBuf,
    /// Budget for each request. A timeout kills the engine.
    pub timeout: Duration,
    /// Start a new engine when the old one has exited.
    pub restart: bool,
}

impl Default for Config {
    fn default() -> Self {
        let root = repo_root();
        Config {
            engine: vec![env::var("INCREMENTAL_ENGINE")
                .unwrap_or_else(|_| root.join("core/dist/incremental").display().to_string())],
            store: env::var_os("INCREMENTAL_STORE")
                .map_or_else(|| root.join(".incremental"), PathBuf::from),
            timeout: Duration::from_secs(10),
            restart: true,
            root,
        }
    }
}

fn repo_root() -> PathBuf {
    let here = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match here.ancestors().find(|dir| dir.join(".git").exists()) {
        Some(dir) => dir.to_path_buf(),
        None => here.clone(),
    }
}

/// The options of an `affected` call (PROTOCOL.md 9.3). `context`, `env`,
/// `config` and `declared_tasks` are sent only when set.
#[derive(Debug, Clone)]
pub struct Affected {
    pub baseline: String,
    pub context: Option<Value>,
    pub verify: bool,
    pub policy: String,
    pub env: Option<Value>,
    pub config: Option<Value>,
    pub declared_tasks: Option<Value>,
}

impl Affected {
    pub fn new(baseline: impl Into<String>) -> Self {
        Affected {
            baseline: baseline.into(),
            context: None,
            verify: false,
            policy: "balanced".to_string(),
            env: None,
            config: None,
            declared_tasks: None,
        }
    }

    /// The `options` object of the request.
    pub fn options(&self) -> Value {
        let mut opts =
            json!({"baseline": self.baseline, "verify": self.verify, "policy": self.policy});
        let map = opts.as_object_mut().expect("options is an object");
        let extra = [
            ("context", &self.context),
            ("env", &self.env),
            ("config", &self.config),
            ("declared_tasks", &self.declared_tasks),
        ];
        for (key, value) in extra {
            if let Some(value) = value {
                map.insert(key.to_string(), value.clone());
            }
        }
        opts
    }
}

/// A running engine and the reader threads for its stdout and stderr.
#[derive(Debug)]
struct Run {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Option<String>>,
    stderr: Arc<Mutex<Vec<String>>>,
    hello: Value,
}

/// A client for one engine process. The process is started by `connect` and
/// restarted on the next call if it died (when `Config::restart` is set).
#[derive(Debug)]
pub struct Incremental {
    cfg: Config,
    run: Option<Run>,
    started: bool,
    next_id: u64,
}

impl Incremental {
    /// Starts the engine and runs the `hello` handshake.
    pub fn connect(cfg: Config) -> Result<Self> {
        let mut client = Incremental {
            cfg,
            run: None,
            started: false,
            next_id: 1,
        };
        client.ensure()?;
        Ok(client)
    }

    /// The `hello` result: `{protocol, engine, capabilities, store}`.
    pub fn hello(&mut self) -> Result<Value> {
        Ok(self.ensure()?.hello.clone())
    }

    pub fn observe(&mut self, observation: &Value) -> Result<Value> {
        self.request("observe", &json!({"observation": observation}))
    }

    /// Atomic: either all observations are stored or none. Returns `{accepted: n}`.
    pub fn observe_many(&mut self, observations: &[Value]) -> Result<Value> {
        self.request("observe_many", &json!({"observations": observations}))
    }

    pub fn baseline(&mut self) -> Result<Value> {
        self.request("baseline", &json!({}))
    }

    /// Fails with `baseline_conflict` unless `expected` equals the current baseline.
    pub fn advance(&mut self, to: &str, expected: Option<&str>) -> Result<Value> {
        self.request("advance", &json!({"to": to, "expected": expected}))
    }

    /// Returns the plan (PROTOCOL.md 9.6). A partial plan is an error.
    pub fn affected(&mut self, changes: &Value, options: &Affected) -> Result<Value> {
        let plan = self.request(
            "affected",
            &json!({"changes": changes, "options": options.options()}),
        )?;
        check_plan(plan)
    }

    pub fn explain(&mut self, task: &str) -> Result<Value> {
        self.request("explain", &json!({"task": task}))
    }

    pub fn forget(&mut self, task: &str) -> Result<Value> {
        self.request("forget", &json!({"task": task}))
    }

    pub fn stats(&mut self) -> Result<Value> {
        self.request("stats", &json!({}))
    }

    /// Forwarded to the engine, which may refuse (`compact` is not in this milestone).
    pub fn compact(&mut self) -> Result<Value> {
        self.request("compact", &json!({}))
    }

    pub fn ping(&mut self) -> Result<Value> {
        self.request("ping", &json!({}))
    }

    /// Sends `shutdown`, waits up to the request timeout for the exit, then kills the engine.
    pub fn close(self) {
        drop(self);
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // The running engine, starting one when there is none or the last one exited.
    fn ensure(&mut self) -> Result<&mut Run> {
        let exited = match self.run.as_mut() {
            Some(run) => matches!(run.child.try_wait(), Ok(Some(_))),
            None => true,
        };
        if exited {
            self.run = None;
            if self.started && !self.cfg.restart {
                return Err(Error::new(
                    "engine_exited",
                    "the engine exited and restart is disabled",
                    None,
                ));
            }
            let run = self.start()?;
            self.run = Some(run);
        }
        Ok(self.run.as_mut().expect("engine is running"))
    }

    fn start(&mut self) -> Result<Run> {
        let mut run = spawn(&self.cfg)?;
        self.started = true;
        let params = json!({"protocol": PROTOCOL, "client": {"name": CLIENT_NAME, "version": CLIENT_VERSION}});
        let id = self.take_id();
        let hello = match call(&mut run, id, "hello", &params, STARTUP_TIMEOUT) {
            Ok(hello) => hello,
            Err(err) => {
                kill(&mut run.child);
                return Err(err);
            }
        };
        match hello.get("protocol").and_then(Value::as_u64) {
            Some(PROTOCOL) => {}
            Some(_) => {
                kill(&mut run.child);
                return Err(Error::new(
                    "protocol_mismatch",
                    "the engine does not speak protocol 1",
                    Some(hello),
                ));
            }
            None => {
                kill(&mut run.child);
                return Err(Error::new(
                    "internal",
                    "the engine answered hello with no protocol",
                    None,
                ));
            }
        }
        run.hello = hello;
        Ok(run)
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value> {
        match self.exchange(method, params) {
            Err(err)
                if err.code == "engine_exited"
                    && self.cfg.restart
                    && READ_ONLY.contains(&method) =>
            {
                self.exchange(method, params)
            }
            other => other,
        }
    }

    fn exchange(&mut self, method: &str, params: &Value) -> Result<Value> {
        let id = self.take_id();
        let timeout = self.cfg.timeout;
        let run = self.ensure()?;
        let result = call(run, id, method, params, timeout);
        if matches!(&result, Err(err) if err.code == "timeout" || err.code == "engine_exited") {
            self.run = None;
        }
        result
    }

    fn stop(&mut self) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        if matches!(run.child.try_wait(), Ok(None)) {
            let id = self.take_id();
            // The engine writes its index image before it exits, so give it time.
            let _ = call(&mut run, id, "shutdown", &json!({}), self.cfg.timeout);
        }
        if wait_exit(&mut run.child, self.cfg.timeout).is_none() {
            kill(&mut run.child);
        }
    }
}

impl Drop for Incremental {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn(cfg: &Config) -> Result<Run> {
    let (cmd, prefix) = cfg
        .engine
        .split_first()
        .ok_or_else(|| Error::new("internal", "the engine command is empty", None))?;
    let mut child = Command::new(cmd)
        .args(prefix)
        .arg("serve")
        .arg("--store")
        .arg(&cfg.store)
        .arg("--root")
        .arg(&cfg.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| Error::new("internal", format!("cannot run the engine: {err}"), None))?;
    let stdin = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    let (tx, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
        {
            if tx.send(Some(line)).is_err() {
                return;
            }
        }
        let _ = tx.send(None);
    });

    let tail = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&tail);
    thread::spawn(move || {
        for line in BufReader::new(stderr)
            .lines()
            .map_while(std::io::Result::ok)
        {
            let mut kept = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            kept.push(line);
            let excess = kept.len().saturating_sub(STDERR_LINES);
            kept.drain(..excess);
        }
    });

    Ok(Run {
        child,
        stdin,
        lines,
        stderr: tail,
        hello: Value::Null,
    })
}

// Sends one request and reads lines until the answer with `id` arrives. A
// timeout or the end of the output ends the run; the caller drops it.
fn call(run: &mut Run, id: u64, method: &str, params: &Value, timeout: Duration) -> Result<Value> {
    let line = json!({"id": id, "method": method, "params": params}).to_string();
    // A failed write means the engine is gone. Its end of output is reported below.
    let _ = writeln!(run.stdin, "{line}");
    let _ = run.stdin.flush();

    let deadline = Instant::now() + timeout;
    loop {
        let raw = match run
            .lines
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(Some(raw)) => raw,
            Ok(None) | Err(RecvTimeoutError::Disconnected) => return Err(exited(run)),
            Err(RecvTimeoutError::Timeout) => {
                kill(&mut run.child);
                let data = json!({"partial": false});
                return Err(Error::new(
                    "timeout",
                    format!("no response within {timeout:?}"),
                    Some(data),
                ));
            }
        };
        let msg: Map<String, Value> = match serde_json::from_str(&raw) {
            Ok(Value::Object(msg)) => msg,
            _ => {
                let text = format!("engine wrote a line that is not a JSON object: {raw:?}");
                return Err(Error::new("bad_response", text, None));
            }
        };
        if msg.get("id") == Some(&json!(id)) {
            if let Some(err) = msg.get("error") {
                return Err(error_of(err));
            }
            return msg.get("result").cloned().ok_or_else(|| {
                Error::new(
                    "bad_response",
                    "response has neither result nor error",
                    None,
                )
            });
        }
        // A store-level failure is printed without an id (CLI style): {"ok": false, "error": {...}}.
        if msg.get("id").is_none_or(|v| v.is_null()) && msg.get("ok") == Some(&Value::Bool(false)) {
            if let Some(err) = msg.get("error") {
                return Err(error_of(err));
            }
        }
        // Any other line belongs to an earlier request and is dropped.
    }
}

fn error_of(err: &Value) -> Error {
    Error {
        code: err
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("internal")
            .to_string(),
        message: err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        data: err.get("data").filter(|data| !data.is_null()).cloned(),
    }
}

// The engine ended while a request was in flight, or before one was sent.
fn exited(run: &mut Run) -> Error {
    let status = wait_exit(&mut run.child, Duration::from_secs(5)).and_then(|status| status.code());
    kill(&mut run.child);
    let stderr = run
        .stderr
        .lock()
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();
    let code = status.map_or_else(|| "unknown".to_string(), |code| code.to_string());
    let data = (!stderr.is_empty()).then(|| Value::String(stderr));
    Error::new(
        "engine_exited",
        format!("engine exited (status {code})"),
        data,
    )
}

fn wait_exit(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => return None,
        }
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

// A plan is never returned partially (PROTOCOL.md 9.6).
fn check_plan(plan: Value) -> Result<Value> {
    let lists = ["tasks", "unobserved", "ignored", "deleted_tasks"];
    let fields = ["fallback", "confidence", "graph_version"];
    let complete = lists
        .iter()
        .all(|key| plan.get(*key).is_some_and(|v| v.is_array()))
        && fields.iter().all(|key| plan.get(*key).is_some());
    if complete {
        Ok(plan)
    } else {
        let data = json!({"reason": "partial_plan"});
        Err(Error::new(
            "internal",
            "the engine answered affected with an incomplete plan",
            Some(data),
        ))
    }
}
