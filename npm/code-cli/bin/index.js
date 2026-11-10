#!/usr/bin/env node
// Frontal Code CLI launcher.
//
// Resolves the native `frontal-code` binary (vendored download or local cargo build)
// and execs it, forwarding argv, stdio, and signals. Dependency-free.

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { downloadRelease } from "../lib/download.mjs";
import { resolveBinary } from "../lib/resolve-binary.mjs";

const PKG_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

function fail(message) {
  process.stderr.write(`frontal-code: ${message}\n`);
  process.stderr.write(
    `\nIf this is unexpected, try:\n` +
      `  npm rebuild -g @frontal-labs/code-cli\n` +
      `  # or, inside the repo:\n` +
      `  (cd cli && ./scripts/download.sh)\n`,
  );
  process.exit(1);
}

// npm may not run install scripts (npm 11 asks for an explicit allow), so the native binary can be
// missing after install. Fetch it on first run instead. Dev builds are never downloaded.
async function fetchOnFirstRun() {
  const version = JSON.parse(readFileSync(join(PKG_ROOT, "package.json"), "utf8")).version;
  if (!version || /dev$/.test(version) || version === "0.0.0") return null;
  process.stderr.write(
    `frontal-code: native binary missing; downloading ${version} (first run)...\n`,
  );
  try {
    await downloadRelease({
      version,
      destDir: PKG_ROOT,
      log: (line) => process.stderr.write(`${line}\n`),
    });
    return resolveBinary();
  } catch (error) {
    process.stderr.write(`frontal-code: download failed: ${error.message}\n`);
    return null;
  }
}

async function main() {
  let binary = resolveBinary();
  if (!binary) {
    binary = await fetchOnFirstRun();
  }
  if (!binary) {
    fail(
      "native `frontal-code` binary not found and could not be downloaded.\n" +
        "Check your network, or build it locally with `cargo build --release -p cli`, then re-run.",
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
}

main();

function osSignalNumber(signal) {
  const map = { SIGINT: 2, SIGTERM: 15, SIGHUP: 1 };
  return map[signal];
}
