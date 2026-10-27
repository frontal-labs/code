import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { detectTarget } from "../lib/platform.mjs";
import {
  __setPkgRootForTests,
  resolveBinary,
  resolveLocalBuild,
  resolveVendoredBinary,
} from "../lib/resolve-binary.mjs";

function makeFakeBin(dir, name = "frontal-code") {
  mkdirSync(dir, { recursive: true });
  const p = join(dir, name);
  writeFileSync(p, "#!/bin/sh\necho hi\n");
  chmodSync(p, 0o755);
  return p;
}

// Lays out a fake monorepo: <repo>/Cargo.toml (workspace) and <repo>/npm/code-cli/.
// Returns the package root that the resolver should treat as its own.
function makeMonorepo(repo) {
  mkdirSync(join(repo, "npm", "code-cli"), { recursive: true });
  writeFileSync(join(repo, "Cargo.toml"), '[workspace]\nmembers = ["cli"]\n');
  writeFileSync(join(repo, "npm", "code-cli", "package.json"), "{}\n");
  return join(repo, "npm", "code-cli");
}

function withTempDir(fn) {
  const dir = mkdtempSync(join(tmpdir(), "frontal-code-"));
  try {
    return fn(dir);
  } finally {
    __setPkgRootForTests(null);
    rmSync(dir, { recursive: true, force: true });
  }
}

test("resolveVendoredBinary finds vendor/<target>/bin/frontal-code", () => {
  withTempDir((root) => {
    const { target, binName } = detectTarget();
    const bin = makeFakeBin(join(root, "vendor", target, "bin"), binName);
    assert.equal(resolveVendoredBinary(root), bin);
  });
});

test("resolveLocalBuild finds target/release in the monorepo that contains the package", () => {
  withTempDir((repo) => {
    const { binName } = detectTarget();
    const pkg = makeMonorepo(repo);
    const bin = makeFakeBin(join(repo, "target", "release"), binName);
    assert.equal(resolveLocalBuild(pkg), bin);
  });
});

test("resolveLocalBuild finds a debug build in the monorepo", () => {
  withTempDir((repo) => {
    const { binName } = detectTarget();
    const pkg = makeMonorepo(repo);
    const bin = makeFakeBin(join(repo, "target", "debug"), binName);
    assert.equal(resolveLocalBuild(pkg), bin);
  });
});

test("resolveLocalBuild returns null when no target build is present", () => {
  withTempDir((repo) => {
    const pkg = makeMonorepo(repo);
    assert.equal(resolveLocalBuild(pkg), null);
  });
});

test("resolveLocalBuild ignores a planted binary in an ancestor without the monorepo marker", () => {
  withTempDir((base) => {
    const { binName } = detectTarget();
    // A cloned/user project with a target/release binary, and the package nested
    // beneath it (e.g. node_modules). Nothing marks this as the Frontal monorepo.
    const project = join(base, "project");
    makeFakeBin(join(project, "target", "release"), binName);
    const pkg = join(project, "node_modules", "@frontal-labs", "code-cli");
    mkdirSync(pkg, { recursive: true });
    assert.equal(resolveLocalBuild(pkg), null);
    assert.equal(resolveBinary(pkg), null);
  });
});

test("resolveLocalBuild ignores a planted binary when Cargo.toml is not a workspace", () => {
  withTempDir((repo) => {
    const { binName } = detectTarget();
    const pkg = makeMonorepo(repo);
    writeFileSync(join(repo, "Cargo.toml"), '[package]\nname = "evil"\n');
    makeFakeBin(join(repo, "target", "release"), binName);
    assert.equal(resolveLocalBuild(pkg), null);
  });
});

test("resolveLocalBuild does not walk up from a nested directory", () => {
  withTempDir((repo) => {
    const { binName } = detectTarget();
    const pkg = makeMonorepo(repo);
    makeFakeBin(join(repo, "target", "release"), binName);
    assert.equal(resolveLocalBuild(join(pkg, "lib")), null);
  });
});

test("resolveBinary prefers vendored over local build", () => {
  withTempDir((repo) => {
    const { target, binName } = detectTarget();
    const pkg = makeMonorepo(repo);
    const vendored = makeFakeBin(join(pkg, "vendor", target, "bin"), binName);
    makeFakeBin(join(repo, "target", "release"), binName);
    assert.equal(resolveBinary(pkg), vendored);
  });
});

test("resolveBinary falls back to local build when no vendor", () => {
  withTempDir((repo) => {
    const { binName } = detectTarget();
    const pkg = makeMonorepo(repo);
    const local = makeFakeBin(join(repo, "target", "release"), binName);
    assert.equal(resolveBinary(pkg), local);
  });
});

test("resolveBinary returns null when nothing present", () => {
  withTempDir((repo) => {
    const pkg = makeMonorepo(repo);
    assert.equal(resolveBinary(pkg), null);
  });
});
