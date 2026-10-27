// Checksums pinned into the npm package at publish time.
//
// `checksums.json` is written by `scripts/pin-checksums.mjs` from the release binaries
// during `prepublishOnly`, so it ships inside the npm tarball. The launcher checks a
// downloaded binary against these hashes as well as the release's `.sha256` sidecar.
// A tampered GitHub release that replaces a binary and its sidecar still fails here,
// because the npm package was published from the genuine release.

import { readFileSync } from "node:fs";

export const CHECKSUMS_URL = new URL("../checksums.json", import.meta.url);

// Load the pinned file. Returns null when it is absent (for example, a source checkout
// that has not been through the publish step).
export function loadPinnedChecksums(url = CHECKSUMS_URL) {
  try {
    return JSON.parse(readFileSync(url, "utf8"));
  } catch (e) {
    if (e.code === "ENOENT") return null;
    throw e;
  }
}

// Return the pinned SHA-256 for one asset of one version. Throws when the pinned file is
// for another version or has no entry for the asset.
export function pinnedSha256(pinned, version, assetName) {
  if (!pinned) {
    throw new Error(
      `No pinned checksums are bundled with this package, so ${assetName} cannot be verified. ` +
        `Reinstall a published release, or build from source with \`cargo build --release -p cli\`.`,
    );
  }
  if (pinned.version !== version) {
    throw new Error(
      `Bundled checksums are for v${pinned.version}, not v${version}. Reinstall the package for this version.`,
    );
  }
  const hex = pinned.assets?.[assetName];
  if (!hex) {
    throw new Error(`No pinned SHA-256 for ${assetName} in v${version}.`);
  }
  return hex;
}
