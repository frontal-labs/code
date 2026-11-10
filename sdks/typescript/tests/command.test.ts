import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { defaultCommand } from "../src/command.js";

const dirs: string[] = [];

function binDir(...names: string[]): string {
  const dir = mkdtempSync(join(tmpdir(), "frontal-code-bin-"));
  dirs.push(dir);
  for (const name of names) {
    const file = join(dir, name);
    writeFileSync(file, "#!/bin/sh\n");
    chmodSync(file, 0o755);
  }
  return dir;
}

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe.skipIf(process.platform === "win32")("defaultCommand", () => {
  it("prefers frontal-code when it is on PATH", () => {
    const dir = binDir("frontal-code", "code");
    expect(defaultCommand({ PATH: dir })).toBe("frontal-code");
  });

  it("falls back to code when only code is on PATH", () => {
    const dir = binDir("code");
    expect(defaultCommand({ PATH: dir })).toBe("code");
  });

  it("ignores a file that is not executable", () => {
    const dir = binDir("code");
    chmodSync(join(dir, "code"), 0o644);
    expect(defaultCommand({ PATH: dir })).toBe("frontal-code");
  });

  it("returns frontal-code when neither name is on PATH", () => {
    expect(defaultCommand({ PATH: binDir() })).toBe("frontal-code");
  });
});
