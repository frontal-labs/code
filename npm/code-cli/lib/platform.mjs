// Platform detection and release-asset mapping for the Frontal Code CLI launcher.
//
// This module maps the current Node platform/arch to the per-platform release
// asset names produced by `.github/workflows/release.yml` (and consumed by
// `homebrew/Formula/code.rb`). It deliberately has no side effects so it is unit
// testable.

export class UnsupportedPlatformError extends Error {
  constructor(platform, arch) {
    super(
      `Unsupported platform "${platform}/${arch}". ` +
        `Frontal Code supports: aarch64-apple-darwin, x86_64-apple-darwin, ` +
        `x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-pc-windows-msvc.`,
    );
    this.name = "UnsupportedPlatformError";
    this.platform = platform;
    this.arch = arch;
  }
}

// Map of "<platform>/<arch>" -> Rust target triple. Release assets are raw
// binaries named `code-cli-<triple>` (plus `.exe` on Windows).
const TARGETS = {
  "darwin/arm64": { target: "aarch64-apple-darwin", ext: "" },
  "darwin/x64": { target: "x86_64-apple-darwin", ext: "" },
  "linux/x64": { target: "x86_64-unknown-linux-gnu", ext: "" },
  "linux/arm64": { target: "aarch64-unknown-linux-gnu", ext: "" },
  "win32/x64": { target: "x86_64-pc-windows-msvc", ext: ".exe" },
};

// Pure helper: resolve a descriptor from explicit platform/arch.
export function targetFor(platform, arch) {
  const entry = TARGETS[`${platform}/${arch}`];
  if (!entry) {
    throw new UnsupportedPlatformError(platform, arch);
  }
  return {
    target: entry.target,
    ext: entry.ext,
    assetName: `code-cli-${entry.target}${entry.ext}`,
    // Binary name inside vendor/<target>/bin/ (the command the launcher execs).
    binName: platform === "win32" ? "frontal-code.exe" : "frontal-code",
  };
}

// Resolve from the running process. Throws `UnsupportedPlatformError` on
// platforms/architectures we do not ship for.
export function detectTarget() {
  return targetFor(process.platform, process.arch);
}
