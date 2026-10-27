// Downloads the native `frontal-code` binary from a GitHub release into vendor/.
//
// Release assets are raw binaries named `code-cli-<target>[.exe]` with a
// `.sha256` sidecar. Dependency-free: Node stdlib only.
//
// No top-level side effects — everything is exported as functions so the
// module is safe to import in tests.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { loadPinnedChecksums, pinnedSha256 } from "./pinned-checksums.mjs";
import { detectTarget, UnsupportedPlatformError } from "./platform.mjs";

export const RELEASE_BASE = "https://github.com/frontal-labs/code/releases/download";

// Build the release asset URL for a given version + target descriptor.
export function releaseAssetUrl(version, assetName) {
  const tag = `v${version}`;
  return `${RELEASE_BASE}/${tag}/${assetName}`;
}

export async function fetchUrl(url) {
  const res = await fetch(url, { redirect: "follow" });
  if (res.status === 404) {
    const err = new Error(`Release asset not found: ${url}`);
    err.code = "NOT_FOUND";
    throw err;
  }
  if (!res.ok) {
    throw new Error(`Download failed (${res.status}) for ${url}`);
  }
  const buf = Buffer.from(await res.arrayBuffer());
  return buf;
}

// Verify a buffer against a sha256 hex string (case-insensitive).
export function verifySha256(buf, expectedHex) {
  const actual = createHash("sha256").update(buf).digest("hex").toLowerCase();
  return actual === String(expectedHex).trim().toLowerCase();
}

// Download and read the `.sha256` sidecar for an asset, if it exists.
export async function fetchSha256(version, assetName) {
  try {
    const buf = await fetchUrl(releaseAssetUrl(version, `${assetName}.sha256`));
    return buf.toString("utf8").trim().split(/\s+/)[0] || null;
  } catch (e) {
    if (e.code === "NOT_FOUND") return null;
    throw e;
  }
}

async function downloadTo({ version, destDir, log = () => undefined, pinned }) {
  const { target, assetName, binName } = detectTarget();
  const url = releaseAssetUrl(version, assetName);
  log(`Downloading ${assetName} (${target}) from ${url}`);
  const buf = await fetchUrl(url);

  // Every published asset ships a .sha256 sidecar. A missing one means the
  // release is incomplete or the download was tampered with, so refuse rather
  // than installing an unverified binary.
  const expected = await fetchSha256(version, assetName);
  if (!expected) {
    throw new Error(
      `No SHA-256 sidecar published for ${assetName}. Refusing to install an ` +
        `unverified binary. If this is a genuine release, report it as a ` +
        `packaging bug; otherwise build from source with ` +
        `\`cargo build --release -p cli\`.`,
    );
  }
  if (!verifySha256(buf, expected)) {
    throw new Error(`SHA-256 mismatch for ${assetName}`);
  }
  // The sidecar comes from the same release as the binary, so it cannot catch a release
  // that was replaced wholesale. The checksum pinned at publish time can.
  if (!verifySha256(buf, pinnedSha256(pinned, version, assetName))) {
    throw new Error(
      `SHA-256 for ${assetName} does not match the checksum pinned in this npm package. ` +
        `The release may have been altered after publish; refusing to install it.`,
    );
  }
  log(`Verified SHA-256 for ${assetName} against the pinned checksum`);

  const binDir = join(destDir, "vendor", target, "bin");
  mkdirSync(binDir, { recursive: true });
  const binPath = join(binDir, binName);
  writeFileSync(binPath, buf);
  return binPath;
}

// Public entry point. Returns the resolved binary path under vendor/.
export async function downloadRelease({
  version,
  destDir,
  log = () => undefined,
  pinned = loadPinnedChecksums(),
}) {
  if (!version || /dev$/.test(version) || version === "0.0.0") {
    throw new Error(
      `Refusing to download for dev version "${version}". Use a tagged release version.`,
    );
  }
  let binPath;
  try {
    binPath = await downloadTo({ version, destDir, log, pinned });
  } catch (e) {
    if (e instanceof UnsupportedPlatformError) throw e;
    throw e;
  }
  // Make executable on POSIX.
  if (process.platform !== "win32" && existsSync(binPath)) {
    try {
      execFileSync("chmod", ["+x", binPath]);
    } catch {
      /* best effort */
    }
  }
  return binPath;
}
