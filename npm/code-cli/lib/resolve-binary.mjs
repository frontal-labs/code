// Resolves the native `frontal-code` binary for the launcher.
//
// Resolution order:
//   1. Vendored binary downloaded during postinstall:
//      <pkgRoot>/vendor/<target>/bin/frontal-code(.exe)
//   2. Local cargo build in the monorepo (dev/workspace case):
//      walk up from <pkgRoot> looking for <repo>/target/release/frontal-code(.exe)
//   3. Otherwise null (caller decides what to do).

import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { detectTarget } from "./platform.mjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
// lib/ -> npm/code-cli/
export const PKG_ROOT = resolve(__dirname, "..");

// Allow tests to override the package root without touching the filesystem.
let ROOT_OVERRIDE = null;
export function __setPkgRootForTests(root) {
  ROOT_OVERRIDE = root;
}
function pkgRoot() {
  return ROOT_OVERRIDE ?? PKG_ROOT;
}

function isExecutableFile(p) {
  if (!existsSync(p)) return false;
  try {
    const s = statSync(p);
    if (!s.isFile()) return false;
    // On Windows `stat` mode bits are not meaningful; treat as executable.
    if (process.platform === "win32") return true;
    // Check any execute bit is set (owner/group/other).
    return Boolean(s.mode & 0o111);
  } catch {
    return false;
  }
}

export function vendorPath(root = pkgRoot()) {
  const { target, binName } = detectTarget();
  return join(root, "vendor", target, "bin", binName);
}

export function resolveVendoredBinary(root = pkgRoot()) {
  const p = vendorPath(root);
  return isExecutableFile(p) ? p : null;
}

// The only repo layout we trust for a local build is this package living inside
// the Frontal Code monorepo: <repo>/npm/code-cli, with <repo>/Cargo.toml declaring
// a workspace. Anything else (including ancestors of a user's project when the
// package is installed under node_modules) is ignored, so a planted
// target/<profile>/frontal-code in a cloned repo cannot be executed.
function monorepoRootFor(pkgDir) {
  const repo = resolve(pkgDir, "..", "..");
  const cargoToml = join(repo, "Cargo.toml");
  const marker = join(repo, "npm", "code-cli", "package.json");
  if (resolve(pkgDir) !== join(repo, "npm", "code-cli")) return null;
  if (!existsSync(cargoToml) || !existsSync(marker)) return null;
  try {
    if (!/^\s*\[workspace\]/m.test(readFileSync(cargoToml, "utf8"))) return null;
  } catch {
    return null;
  }
  return repo;
}

// Look for a locally built `target/<profile>/<binName>` in the monorepo root that
// contains this package. Checks both `release` and `debug` so a plain `cargo build`
// (debug) works too.
export function resolveLocalBuild(startDir = pkgRoot()) {
  const { binName } = detectTarget();
  const repo = monorepoRootFor(resolve(startDir));
  if (!repo) return null;
  for (const profile of ["release", "debug"]) {
    const candidate = join(repo, "target", profile, binName);
    if (isExecutableFile(candidate)) return candidate;
  }
  return null;
}

// Full resolution: vendored first, then local cargo build.
export function resolveBinary(startDir = pkgRoot()) {
  return resolveVendoredBinary(startDir) ?? resolveLocalBuild(startDir);
}
