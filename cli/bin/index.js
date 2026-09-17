#!/usr/bin/env node
// Frontal Code CLI launcher.
//
// Resolves the native `frontal-code` binary (vendored download or local cargo build)
// and execs it, forwarding argv, stdio, and signals. Dependency-free.

import { spawn } from "node:child_process";
import { resolveBinary } from "../lib/resolve-binary.mjs";

function fail(message) {
  process.stderr.write(`frontal-code: ${message}\n`);
  process.stderr.write(
    `\nIf this is unexpected, try:\n` +
      `  npm rebuild @frontal-labs/frontal-code\n` +
      `  # or, inside the repo:\n` +
      `  (cd cli && ./scripts/download.sh)\n`,
  );
  process.exit(1);
}

const binary = resolveBinary();
if (!binary) {
  fail(
    "native `frontal-code` binary not found.\n" +
      "The postinstall step may have been skipped (offline/CI) or failed to download.\n" +
      "Build it locally with `cargo build --release -p frontal-code-cli`, then re-run.",
  );
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

// Forward termination signals so the child receives them (e.g. Ctrl-C).
for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => {
    if (!child.killed) child.kill(sig);
  });
}

child.on("error", (err) => {
  fail(`failed to start native binary: ${err.message}`);
});

child.on("exit", (code, signal) => {
  if (signal) {
    // Mimic shell: exit with 128 + signal number.
    process.exit(128 + (osSignalNumber(signal) ?? 0));
  }
  process.exit(code ?? 1);
});

function osSignalNumber(signal) {
  const map = { SIGINT: 2, SIGTERM: 15, SIGHUP: 1 };
  return map[signal];
}
