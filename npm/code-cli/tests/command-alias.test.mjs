import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ALIAS = resolve(__dirname, "..", "scripts", "command-alias.sh");
const SKIP = process.platform === "win32";

function fakeBin(dir, name) {
  mkdirSync(dir, { recursive: true });
  const file = join(dir, name);
  writeFileSync(file, "#!/bin/sh\necho fake\n");
  chmodSync(file, 0o755);
  return file;
}

function run(args, pathDirs) {
  return spawnSync("bash", [ALIAS, ...args], {
    env: { PATH: [...pathDirs, "/usr/bin", "/bin"].join(":") },
    encoding: "utf8",
  });
}

test("install creates code beside frontal-code", { skip: SKIP }, () => {
  const root = mkdtempSync(join(tmpdir(), "frontal-code-alias-"));
  try {
    const bin = join(root, "npm-bin");
    const fc = fakeBin(bin, "frontal-code");
    const res = run(["install"], [bin]);
    assert.equal(res.status, 0, res.stderr);
    const code = join(bin, "code");
    assert.ok(lstatSync(code).isSymbolicLink(), "code should be a symlink");
    assert.equal(readlinkSync(code), fc);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("install never replaces another code command", { skip: SKIP }, () => {
  const root = mkdtempSync(join(tmpdir(), "frontal-code-alias-"));
  try {
    const bin = join(root, "npm-bin");
    fakeBin(bin, "frontal-code");
    const other = join(root, "vscode-bin");
    const vscodeCode = fakeBin(other, "code");
    const res = run(["install"], [other, bin]);
    assert.equal(res.status, 0, res.stderr);
    assert.match(res.stdout, /not replacing it/);
    assert.equal(existsSync(join(bin, "code")), false, "no alias should be created");
    assert.ok(existsSync(vscodeCode));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("install does nothing when frontal-code is not on PATH", { skip: SKIP }, () => {
  const root = mkdtempSync(join(tmpdir(), "frontal-code-alias-"));
  try {
    const res = run(["install"], [join(root, "empty")]);
    assert.equal(res.status, 0, res.stderr);
    assert.match(res.stdout, /not on PATH yet/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("remove deletes only the alias it created", { skip: SKIP }, () => {
  const root = mkdtempSync(join(tmpdir(), "frontal-code-alias-"));
  try {
    const bin = join(root, "npm-bin");
    const fc = fakeBin(bin, "frontal-code");
    symlinkSync(fc, join(bin, "code"));
    const res = run(["remove"], [bin]);
    assert.equal(res.status, 0, res.stderr);
    assert.equal(existsSync(join(bin, "code")), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("remove keeps a code command it did not create", { skip: SKIP }, () => {
  const root = mkdtempSync(join(tmpdir(), "frontal-code-alias-"));
  try {
    const bin = join(root, "npm-bin");
    fakeBin(bin, "frontal-code");
    const vscodeCode = fakeBin(bin, "code");
    const res = run(["remove"], [bin]);
    assert.equal(res.status, 0, res.stderr);
    assert.ok(existsSync(vscodeCode), "a real code file must survive");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
