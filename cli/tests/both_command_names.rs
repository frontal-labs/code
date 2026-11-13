//! Both command names must behave the same: same output, same exit status.

use std::process::Command;

fn run(bin: &str, args: &[&str]) -> std::process::Output {
    Command::new(bin)
        .args(args)
        .output()
        .expect("binary should run")
}

#[test]
fn frontal_code_prints_the_same_version_as_code() {
    let code = run(env!("CARGO_BIN_EXE_code"), &["--version"]);
    let frontal = run(env!("CARGO_BIN_EXE_frontal-code"), &["--version"]);
    assert!(code.status.success(), "code --version should succeed");
    assert_eq!(frontal.stdout, code.stdout);
}

#[test]
fn frontal_code_passes_the_exit_status_through() {
    // An unknown flag is rejected before any provider call, so the test never reaches the network.
    let args = ["--definitely-not-a-flag"];
    let code = run(env!("CARGO_BIN_EXE_code"), &args);
    let frontal = run(env!("CARGO_BIN_EXE_frontal-code"), &args);
    assert!(!code.status.success());
    assert_eq!(frontal.status.code(), code.status.code());
}
