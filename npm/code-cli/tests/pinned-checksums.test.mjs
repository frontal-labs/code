import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { downloadRelease } from "../lib/download.mjs";
import { pinnedSha256 } from "../lib/pinned-checksums.mjs";
import { allAssetNames, detectTarget } from "../lib/platform.mjs";
import { pinChecksums } from "../scripts/pin-checksums.mjs";

const sha = (buf) => createHash("sha256").update(buf).digest("hex");
const VERSION = "9.9.9";

test("pinnedSha256 refuses when no checksums are bundled", () => {
  assert.throws(() => pinnedSha256(null, VERSION, "code-cli-x"), /No pinned checksums/);
});

test("pinnedSha256 refuses checksums for a different version", () => {
  const pinned = { version: "1.0.0", assets: { "code-cli-x": "aa" } };
  assert.throws(() => pinnedSha256(pinned, VERSION, "code-cli-x"), /are for v1\.0\.0/);
});

test("pinnedSha256 refuses an asset with no entry and returns a matching one", () => {
  const pinned = { version: VERSION, assets: { "code-cli-x": "abc" } };
  assert.equal(pinnedSha256(pinned, VERSION, "code-cli-x"), "abc");
  assert.throws(() => pinnedSha256(pinned, VERSION, "code-cli-y"), /No pinned SHA-256/);
});

test("allAssetNames lists every platform asset once", () => {
  const names = allAssetNames();
  assert.equal(new Set(names).size, names.length);
  assert.equal(names.length, 5);
  assert.ok(names.includes(detectTarget().assetName));
});

test("downloadRelease refuses a binary that matches its sidecar but not the pinned checksum", async () => {
  // A release replaced wholesale: the binary and its sidecar agree with each other, but
  // not with the checksum pinned when the npm package was published.
  const { assetName } = detectTarget();
  const tampered = Buffer.from("tampered binary");
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url) =>
    String(url).endsWith(".sha256")
      ? new Response(`${sha(tampered)}  ${assetName}`, { status: 200 })
      : new Response(tampered, { status: 200 });
  try {
    const pinned = { version: VERSION, assets: { [assetName]: sha(Buffer.from("genuine")) } };
    await assert.rejects(
      downloadRelease({ version: VERSION, destDir: mkdtempSync(join(tmpdir(), "fc-pin-")), pinned }),
      /does not match the checksum pinned/,
    );
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("downloadRelease installs a binary that matches both the sidecar and the pinned checksum", async () => {
  const { assetName } = detectTarget();
  const genuine = Buffer.from("genuine binary");
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url) =>
    String(url).endsWith(".sha256")
      ? new Response(`${sha(genuine)}  ${assetName}`, { status: 200 })
      : new Response(genuine, { status: 200 });
  try {
    const pinned = { version: VERSION, assets: { [assetName]: sha(genuine) } };
    const binPath = await downloadRelease({
      version: VERSION,
      destDir: mkdtempSync(join(tmpdir(), "fc-pin-")),
      pinned,
    });
    assert.ok(binPath.endsWith(detectTarget().binName));
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("pinChecksums hashes the binaries and rejects a sidecar that disagrees", async () => {
  const bytes = Buffer.from("release bytes");
  const ok = await pinChecksums(VERSION, {
    fetchBinary: async () => bytes,
    fetchSidecar: async () => sha(bytes),
  });
  assert.equal(ok.version, VERSION);
  assert.equal(Object.keys(ok.assets).length, allAssetNames().length);
  for (const hex of Object.values(ok.assets)) assert.equal(hex, sha(bytes));

  await assert.rejects(
    pinChecksums(VERSION, {
      fetchBinary: async () => bytes,
      fetchSidecar: async () => sha(Buffer.from("other")),
    }),
    /does not match the binary/,
  );
});
