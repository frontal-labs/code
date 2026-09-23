import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";

export interface MockFrontalCodeOptions {
  /** Raw JSONL the fake binary should emit on stdout. */
  response?: string;
  /** Exit code for the fake binary. */
  exitCode?: number;
  /** Optional path where the fake binary writes the argv it received. */
  captureFile?: string;
  /** Optional path where the fake binary writes its working directory. */
  cwdFile?: string;
  /** Optional binary name (defaults to `frontal-code`). */
  scriptName?: string;
}

/**
 * A hermetic stand-in for the `frontal-code` CLI. It records the argv/cwd it was
 * invoked with and echoes a configurable JSONL payload, so SDK tests need no
 * real binary, API key, or network access.
 */
export class MockFrontalCode {
  readonly dir: string;
  readonly captureFile: string;
  readonly cwdFile: string;
  readonly binPath: string;
  readonly responseFile: string;

  constructor(opts: MockFrontalCodeOptions = {}) {
    this.dir = mkdtempSync(join(tmpdir(), "frontal-code-mock-"));
    this.captureFile = opts.captureFile ?? join(this.dir, "capture.txt");
    this.cwdFile = opts.cwdFile ?? join(this.dir, "cwd.txt");
    this.responseFile = join(this.dir, "response.jsonl");
    writeFileSync(this.responseFile, opts.response ?? "");

    const script = [
      "#!/usr/bin/env bash",
      'CAP="${FRONTAL_CODE_MOCK_CAPTURE:-/dev/null}"',
      'CWD="${FRONTAL_CODE_MOCK_CWD:-/dev/null}"',
      `RESP="${this.responseFile}"`,
      `EXIT="${opts.exitCode ?? 0}"`,
      'pwd > "$CWD"',
      'printf \'%s\\0\' "$@" > "$CAP"',
      'cat "$RESP"',
      'exit "$EXIT"',
      "",
    ].join("\n");

    this.binPath = join(this.dir, opts.scriptName ?? "frontal-code");
    writeFileSync(this.binPath, script);
    chmodSync(this.binPath, 0o755);
  }

  /** Environment values that route a spawned CLI to this mock. */
  env(extra: Record<string, string> = {}): Record<string, string> {
    return {
      FRONTAL_CODE_MOCK_CAPTURE: this.captureFile,
      FRONTAL_CODE_MOCK_CWD: this.cwdFile,
      ...extra,
    };
  }

  /** Prepend this mock's directory to `PATH`. */
  withPath(): string {
    return `${this.dir}${delimiter}${process.env.PATH ?? ""}`;
  }

  /** Read the argv the mock captured, one element per line. */
  capturedArgs(): string[] {
    try {
      return readFileSync(this.captureFile, "utf8")
        .split("\0")
        .filter((line) => line.length > 0);
    } catch {
      return [];
    }
  }

  /** Read the working directory the mock recorded. */
  capturedCwd(): string {
    try {
      return readFileSync(this.cwdFile, "utf8").trim();
    } catch {
      return "";
    }
  }
}
