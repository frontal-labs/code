import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { detectTarget } from "../lib/platform.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = resolve(__dirname, "..");
const SKIP = process.platform === "win32";

// bin/index.js forwards argv + exit code to the resolved binary.
test("bin/index.js delegates to launcher with argv and exit code passthrough", {
  skip: SKIP,
}, () => {
  const { target, binName } = detectTarget();
  const fakeRoot = mkdtempSync(join(tmpdir(), "frontal-code-index-"));
  try {
    cpSync(join(PKG_ROOT, "bin"), join(fakeRoot, "bin"), { recursive: true });
    cpSync(join(PKG_ROOT, "lib"), join(fakeRoot, "lib"), { recursive: true });

    const vendored = join(fakeRoot, "vendor", target, "bin");
    mkdirSync(vendored, { recursive: true });
    const fakeBin = join(vendored, binName);
    writeFileSync(fakeBin, '#!/bin/sh\necho "ARGS:$@"\n[ "$1" = "fail" ] && exit 7\nexit 0\n');
    chmodSync(fakeBin, 0o755);

    const indexLauncher = join(fakeRoot, "bin", "index.js");

    const ok = spawnSync("node", [indexLauncher, "hello", "world"], { encoding: "utf8" });
    assert.match(ok.stdout, /ARGS:hello world/);
    assert.equal(ok.status, 0);

    const fail = spawnSync("node", [indexLauncher, "fail"], { encoding: "utf8" });
    assert.equal(fail.status, 7);
  } finally {
    rmSync(fakeRoot, { recursive: true, force: true });
  }
});
