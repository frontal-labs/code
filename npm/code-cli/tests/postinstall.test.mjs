import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { detectTarget } from "../lib/platform.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = resolve(__dirname, "..");
const POSTINSTALL = join(PKG_ROOT, "scripts", "postinstall.sh");
const SKIP = process.platform === "win32";

// The script finds its package from its own location, so each test runs a copy placed inside its
// fake package. Running the real script would read the real version and exercise the wrong branch.
function installCopy(fakeRoot) {
  mkdirSync(join(fakeRoot, "scripts"), { recursive: true });
  const copy = join(fakeRoot, "scripts", "postinstall.sh");
  writeFileSync(copy, readFileSync(POSTINSTALL));
  chmodSync(copy, 0o755);
  // The script imports its helpers from lib/, relative to the package root.
  cpSync(join(PKG_ROOT, "lib"), join(fakeRoot, "lib"), { recursive: true });
  return copy;
}

// Run the real postinstall in a throwaway package copy with a fake vendored
// binary, asserting dev-version + skip-env short-circuits without network.
test("postinstall skips on FRONTAL_CODE_SKIP_DOWNLOAD without touching network", {
  skip: SKIP,
}, () => {
  const { target, binName } = detectTarget();
  const fakeRoot = mkdtempSync(join(tmpdir(), "frontal-code-pi-"));
  try {
    const vendorDir = join(fakeRoot, "vendor", target, "bin");
    mkdirSync(vendorDir, { recursive: true });
    const fakeBin = join(vendorDir, binName);
    writeFileSync(fakeBin, "#!/bin/sh\necho hi\n");
    chmodSync(fakeBin, 0o755);

    // package.json with a dev version + a real vendored binary already present.
    writeFileSync(
      join(fakeRoot, "package.json"),
      JSON.stringify({ name: "@frontal-labs/code-cli", version: "0.0.0-dev" }),
    );

    const res = spawnSync("bash", [installCopy(fakeRoot)], {
      cwd: fakeRoot,
      env: { ...process.env, FRONTAL_CODE_SKIP_DOWNLOAD: "1", PATH: process.env.PATH },
      encoding: "utf8",
    });
    assert.equal(res.status, 0);
    assert.match(res.stdout, /skipping download/);
  } finally {
    rmSync(fakeRoot, { recursive: true, force: true });
  }
});

test("postinstall skips download for dev version (no network)", { skip: SKIP }, () => {
  const fakeRoot = mkdtempSync(join(tmpdir(), "frontal-code-pi2-"));
  try {
    writeFileSync(
      join(fakeRoot, "package.json"),
      JSON.stringify({ name: "@frontal-labs/code-cli", version: "0.0.0-dev" }),
    );
    const res = spawnSync("bash", [installCopy(fakeRoot)], {
      cwd: fakeRoot,
      env: { ...process.env, PATH: process.env.PATH },
      encoding: "utf8",
    });
    assert.equal(res.status, 0);
    assert.match(res.stdout, /dev version/);
  } finally {
    rmSync(fakeRoot, { recursive: true, force: true });
  }
});

test("postinstall fails gracefully (exit 0) when offline download fails", { skip: SKIP }, () => {
  const fakeRoot = mkdtempSync(join(tmpdir(), "frontal-code-pi3-"));
  try {
    // Real, non-dev version but no network: download will fail; must exit 0.
    writeFileSync(
      join(fakeRoot, "package.json"),
      JSON.stringify({ name: "@frontal-labs/code-cli", version: "9.9.9" }),
    );
    const res = spawnSync("bash", [installCopy(fakeRoot)], {
      cwd: fakeRoot,
      // An unreachable release host makes the download fail without depending on the network.
      env: {
        ...process.env,
        PATH: process.env.PATH,
        FRONTAL_CODE_FORCE_DOWNLOAD: "1",
        FRONTAL_CODE_RELEASE_BASE: "http://127.0.0.1:1/releases",
      },
      encoding: "utf8",
      timeout: 60000,
    });
    assert.equal(res.status, 0);
    assert.match(res.stdout + res.stderr, /failed to download native binary/);
  } finally {
    rmSync(fakeRoot, { recursive: true, force: true });
  }
});
