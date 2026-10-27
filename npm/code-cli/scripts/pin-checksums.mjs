#!/usr/bin/env node
// Writes checksums.json for one release version. Run from prepublishOnly, after the
// GitHub release for `v<version>` exists.
//
// It downloads every platform binary and hashes the bytes. It also checks each hash
// against the release's `.sha256` sidecar, so a mismatch fails the publish instead of
// pinning a bad value.

import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { fetchSha256, fetchUrl, releaseAssetUrl } from "../lib/download.mjs";
import { allAssetNames } from "../lib/platform.mjs";

export async function pinChecksums(
  version,
  { out, fetchBinary = fetchUrl, fetchSidecar = fetchSha256 } = {},
) {
  const assets = {};
  for (const assetName of allAssetNames()) {
    const bytes = await fetchBinary(releaseAssetUrl(version, assetName));
    const actual = createHash("sha256").update(bytes).digest("hex");
    const sidecar = await fetchSidecar(version, assetName);
    if (!sidecar) {
      throw new Error(`No .sha256 sidecar published for ${assetName} in v${version}.`);
    }
    if (sidecar.toLowerCase() !== actual) {
      throw new Error(
        `Sidecar for ${assetName} does not match the binary (sidecar ${sidecar}, binary ${actual}).`,
      );
    }
    assets[assetName] = actual;
  }
  const pinned = { version, assets };
  if (out) writeFileSync(out, `${JSON.stringify(pinned, null, 2)}\n`);
  return pinned;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const version = process.argv[2];
  if (!version || /dev$/.test(version) || version === "0.0.0") {
    console.error("usage: pin-checksums.mjs <release-version>  (refuses dev versions)");
    process.exit(1);
  }
  const out = fileURLToPath(new URL("../checksums.json", import.meta.url));
  await pinChecksums(version, { out });
  console.log(`Pinned ${allAssetNames().length} checksums for v${version} -> checksums.json`);
}
