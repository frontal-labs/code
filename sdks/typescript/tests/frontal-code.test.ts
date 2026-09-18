import { describe, expect, it } from "vitest";
import { FrontalCode } from "../src/frontal-code.js";
import { MockFrontalCode } from "../src/mockCli.js";
import { Thread } from "../src/thread.js";

describe("Frontal Code client", () => {
  it("defaults the CLI command to 'frontal-code'", () => {
    expect(new FrontalCode().command).toBe("frontal-code");
  });

  it("honors a custom command path", () => {
    expect(new FrontalCode({ command: "/usr/bin/frontal-code" }).command).toBe(
      "/usr/bin/frontal-code",
    );
  });

  it("starts and resumes threads", () => {
    const frontal_code = new FrontalCode();
    const fresh = frontal_code.startThread();
    const resumed = frontal_code.resumeThread("sess-abc");

    expect(fresh).toBeInstanceOf(Thread);
    expect(fresh.id).toBeUndefined();
    expect(resumed.id).toBe("sess-abc");
  });

  it("merges process.env with user env", () => {
    const frontal_code = new FrontalCode({ env: { MY_VAR: "value" } });
    const env = frontal_code.buildEnv();
    expect(env.MY_VAR).toBe("value");
    expect(env.PATH).toBeDefined();
  });

  it("injects required variables from the ambient environment", () => {
    const previous = process.env.CODEX_API_KEY;
    process.env.CODEX_API_KEY = "injected-key";
    try {
      const frontal_code = new FrontalCode({ env: {} });
      const env = frontal_code.buildEnv();
      expect(env.CODEX_API_KEY).toBe("injected-key");
    } finally {
      if (previous === undefined) delete process.env.CODEX_API_KEY;
      else process.env.CODEX_API_KEY = previous;
    }
  });

  it("does not overwrite a user-provided required variable", () => {
    const previous = process.env.CODEX_API_KEY;
    process.env.CODEX_API_KEY = "ambient";
    try {
      const frontal_code = new FrontalCode({ env: { CODEX_API_KEY: "explicit" } });
      expect(frontal_code.buildEnv().CODEX_API_KEY).toBe("explicit");
    } finally {
      if (previous === undefined) delete process.env.CODEX_API_KEY;
      else process.env.CODEX_API_KEY = previous;
    }
  });

  it("routes spawned CLI calls to a mock binary", async () => {
    const mock = new MockFrontalCode({
      response: '{"type":"turn.completed","finalResponse":"hi","sessionId":"s1"}',
    });
    const frontal_code = new FrontalCode({
      command: mock.binPath,
      env: mock.env(),
    });
    const turn = await frontal_code.startThread().run("hello");
    expect(turn.finalResponse).toBe("hi");
    expect(mock.capturedArgs()).toContain("hello");
  });
});
