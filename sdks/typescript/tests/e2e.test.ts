import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FrontalCodeEventSchema } from "@frontal-labs/code-zod-schemas";
import { describe, expect, it } from "vitest";
import { FrontalCode } from "../src/frontal-code.js";
import type { FrontalCodeEvent } from "../src/protocol.js";

// Runs against the real `frontal-code` binary and a live model, so it is skipped
// unless the binary is provided. The default provider is a local Ollama server:
//   FRONTAL_CODE_E2E_BIN=/path/to/code
//   OLLAMA_BASE_URL=http://127.0.0.1:11434     (default: http://localhost:11434)
//   FRONTAL_CODE_E2E_MODEL=qwen2.5:1.5b        (default)
// For a cloud OpenAI-compatible provider set FRONTAL_CODE_E2E_PROVIDER=openai and
// OPENAI_API_KEY (and OPENAI_BASE_URL for a non-OpenAI endpoint).
// The mock-based tests cover protocol handling; this proves the wire contract.
const bin = process.env.FRONTAL_CODE_E2E_BIN;
const provider = process.env.FRONTAL_CODE_E2E_PROVIDER ?? "ollama";
const apiKey = process.env.OPENAI_API_KEY;
const enabled = Boolean(bin) && (provider !== "openai" || Boolean(apiKey));

const MODEL = process.env.FRONTAL_CODE_E2E_MODEL ?? "qwen2.5:1.5b";

function client(): FrontalCode {
  return new FrontalCode({
    command: bin,
    env: {
      ...(apiKey ? { OPENAI_API_KEY: apiKey } : {}),
      ...(process.env.OPENAI_BASE_URL ? { OPENAI_BASE_URL: process.env.OPENAI_BASE_URL } : {}),
      ...(process.env.OLLAMA_BASE_URL ? { OLLAMA_BASE_URL: process.env.OLLAMA_BASE_URL } : {}),
      FRONTAL_CODE_MAX_OUTPUT_TOKENS: process.env.FRONTAL_CODE_E2E_MAX_TOKENS ?? "1024",
    },
  });
}

// Sessions are written under the working directory, so keep them out of the package.
const threadOptions = {
  provider,
  model: MODEL,
  permissionMode: "read-only",
  workingDirectory: mkdtempSync(join(tmpdir(), "frontal-code-e2e-")),
} as const;

describe.skipIf(!enabled)("real CLI end-to-end", () => {
  it("runs a turn and returns a session id", async () => {
    const thread = client().startThread(threadOptions);
    const turn = await thread.run("Reply with the single word: pong");
    expect(turn.finalResponse.trim().length).toBeGreaterThan(0);
    expect(thread.id).toBeTruthy();
  }, 240_000);

  it("streams the documented event sequence", async () => {
    const thread = client().startThread(threadOptions);
    const events: FrontalCodeEvent[] = [];
    const streamed = await thread.runStreamed("Reply with the single word: pong");
    for await (const event of streamed.events) {
      // Every event the real CLI produces must match the shared schema.
      FrontalCodeEventSchema.parse(event);
      events.push(event);
    }
    expect(events[0]?.type).toBe("turn.started");
    expect(events.at(-1)?.type).toBe("turn.completed");
  }, 240_000);

  it("resumes a session and keeps earlier context", async () => {
    const frontal = client();
    const first = frontal.startThread(threadOptions);
    await first.run("My favorite color is teal. Acknowledge in one short sentence.");
    expect(first.id).toBeTruthy();

    const resumed = frontal.resumeThread(first.id as string, threadOptions);
    const turn = await resumed.run("What is my favorite color? Answer with one word.");
    expect(turn.finalResponse.toLowerCase()).toContain("teal");
  }, 480_000);
});
