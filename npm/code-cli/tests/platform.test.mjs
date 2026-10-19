import assert from "node:assert/strict";
import { test } from "node:test";
import { detectTarget, targetFor, UnsupportedPlatformError } from "../lib/platform.mjs";

test("detectTarget maps darwin/arm64 to aarch64-apple-darwin", () => {
  const t = targetFor("darwin", "arm64");
  assert.equal(t.target, "aarch64-apple-darwin");
  assert.equal(t.assetName, "code-cli-aarch64-apple-darwin");
  assert.equal(t.binName, "frontal-code");
});

test("detectTarget maps darwin/x64 to x86_64-apple-darwin", () => {
  const t = targetFor("darwin", "x64");
  assert.equal(t.target, "x86_64-apple-darwin");
  assert.equal(t.assetName, "code-cli-x86_64-apple-darwin");
});

test("detectTarget maps linux/x64 to x86_64-unknown-linux-gnu", () => {
  const t = targetFor("linux", "x64");
  assert.equal(t.target, "x86_64-unknown-linux-gnu");
  assert.equal(t.assetName, "code-cli-x86_64-unknown-linux-gnu");
});

test("detectTarget maps linux/arm64 to aarch64-unknown-linux-gnu", () => {
  const t = targetFor("linux", "arm64");
  assert.equal(t.target, "aarch64-unknown-linux-gnu");
  assert.equal(t.assetName, "code-cli-aarch64-unknown-linux-gnu");
});

test("detectTarget maps win32/x64 to x86_64-pc-windows-msvc (.exe)", () => {
  const t = targetFor("win32", "x64");
  assert.equal(t.target, "x86_64-pc-windows-msvc");
  assert.equal(t.assetName, "code-cli-x86_64-pc-windows-msvc.exe");
  assert.equal(t.binName, "frontal-code.exe");
});

test("detectTarget throws UnsupportedPlatformError for unknown", () => {
  assert.throws(() => targetFor("sunos", "sparc"), UnsupportedPlatformError);
});

test("detectTarget (process) returns a valid descriptor on the CI runner", () => {
  const t = detectTarget();
  assert.ok(t.target);
  assert.ok(t.assetName.startsWith("code-cli-"));
});
