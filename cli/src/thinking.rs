//! The animated "Thinking..." line shown while a turn waits for its first output.
//!
//! The line is drawn on the current terminal row and erased before any streamed text,
//! tool headline, or permission prompt is written, so output never lands on top of it.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_INTERVAL: Duration = Duration::from_millis(90);
/// The theme's spinner blue (ANSI 256-color 33).
const COLOR: &str = "\x1b[38;5;33m";

struct Running {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// Starts the animation. Does nothing when stdout is not a terminal.
pub fn start() {
    if !io::stdout().is_terminal() {
        return;
    }
    stop();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_flag = Arc::clone(&stop_flag);
    let started = Instant::now();
    let handle = thread::spawn(move || {
        let mut frame = 0;
        while !thread_flag.load(Ordering::SeqCst) {
            let glyph = FRAMES[frame % FRAMES.len()];
            let seconds = started.elapsed().as_secs();
            let mut out = io::stdout();
            let _ = write!(out, "\r\x1b[2K{COLOR}{glyph} Thinking... {seconds}s\x1b[0m");
            let _ = out.flush();
            frame += 1;
            thread::sleep(FRAME_INTERVAL);
        }
        let mut out = io::stdout();
        let _ = write!(out, "\r\x1b[2K");
        let _ = out.flush();
    });

    if let Ok(mut running) = RUNNING.lock() {
        *running = Some(Running {
            stop: stop_flag,
            handle,
        });
    }
}

/// Stops the animation and erases its line. Safe to call when nothing is running.
pub fn stop() {
    let running = RUNNING.lock().ok().and_then(|mut slot| slot.take());
    if let Some(Running { stop, handle }) = running {
        stop.store(true, Ordering::SeqCst);
        let _ = handle.join();
    }
}
