//! `frontal-code` is the same CLI as `code`. The package builds `code`; this binary runs the
//! `code` that sits beside it. Arguments, stdio, and the exit status pass through unchanged, so
//! scripts and SDKs that call either name see the same behavior.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let target = sibling_code();
    match Command::new(&target)
        .args(std::env::args_os().skip(1))
        .status()
    {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            // Killed by a signal: report failure rather than invent an exit code.
            None => ExitCode::FAILURE,
        },
        Err(error) => {
            eprintln!("frontal-code: cannot run {}: {error}", target.display());
            ExitCode::FAILURE
        }
    }
}

/// The `code` binary in the same directory as this one. Falls back to a PATH lookup when this
/// binary was copied somewhere without its sibling.
fn sibling_code() -> PathBuf {
    let name = if cfg!(windows) { "code.exe" } else { "code" };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}
