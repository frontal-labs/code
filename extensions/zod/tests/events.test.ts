import { describe, expect, it } from "vitest";
import { FrontalCodeEventSchema, parseFrontalCodeEvent } from "../src/index.js";

describe("FrontalCodeEventSchema", () => {
  it.each([
    { type: "turn.started" },
    { type: "item.completed", item: { type: "text", text: "pong" } },
    { type: "item.completed", item: { type: "tool_use", name: "read_file", input: "a.txt" } },
    { type: "item.completed", item: { type: "tool_result", content: "1.0" } },
    { type: "item.completed", item: { type: "image", path: "x.png" } },
    { type: "turn.failed", error: "upstream failed" },
  ])("accepts %j", (event) => {
    expect(FrontalCodeEventSchema.parse(event)).toEqual(event);
  });

  it("accepts a completed turn with usage and a session id", () => {
    const event = {
      type: "turn.completed",
      finalResponse: "pong",
      usage: {
        input_tokens: 31,
        output_tokens: 2,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
      },
      sessionId: "session-1",
    };
    expect(FrontalCodeEventSchema.parse(event)).toEqual(event);
  });

  it("rejects an unknown event type", () => {
    expect(() => FrontalCodeEventSchema.parse({ type: "turn.paused" })).toThrow();
  });

  it("rejects negative or fractional token counts", () => {
    const usage = {
      input_tokens: -1,
      output_tokens: 0,
      cache_creation_input_tokens: 0,
      cache_read_input_tokens: 0,
    };
    expect(() =>
      FrontalCodeEventSchema.parse({ type: "turn.completed", finalResponse: "", usage }),
    ).toThrow();
    const fractional = { ...usage, input_tokens: 1.5 };
    expect(() =>
      FrontalCodeEventSchema.parse({
        type: "turn.completed",
        finalResponse: "",
        usage: fractional,
      }),
    ).toThrow();
  });

  it("rejects an item with a missing field", () => {
    expect(() =>
      FrontalCodeEventSchema.parse({ type: "item.completed", item: { type: "text" } }),
    ).toThrow();
  });
});

describe("parseFrontalCodeEvent", () => {
  it("parses a JSONL line", () => {
    expect(parseFrontalCodeEvent('{"type":"turn.started"}')).toEqual({ type: "turn.started" });
  });

  it("names the failing field in the error", () => {
    expect(() => parseFrontalCodeEvent('{"type":"turn.failed"}')).toThrow(/error/);
  });

  it("reports invalid JSON as a syntax error", () => {
    expect(() => parseFrontalCodeEvent("{not json")).toThrow(SyntaxError);
  });
});
