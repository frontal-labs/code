import { z } from "zod";

/**
 * Zod schemas for the events the Frontal Code SDKs expose. The TypeScript and Rust SDKs parse
 * the CLI's JSONL stream into these shapes. Keeping the schemas here lets every consumer check
 * the same contract, and lets tests fail when the CLI's output changes.
 */

export const UsageSchema = z.object({
  input_tokens: z.number().int().nonnegative(),
  output_tokens: z.number().int().nonnegative(),
  cache_creation_input_tokens: z.number().int().nonnegative(),
  cache_read_input_tokens: z.number().int().nonnegative(),
});

export const ThreadItemSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("text"), text: z.string() }),
  z.object({ type: z.literal("tool_use"), name: z.string(), input: z.string() }),
  z.object({ type: z.literal("tool_result"), content: z.string() }),
  z.object({
    type: z.literal("image"),
    path: z.string().optional(),
    url: z.string().optional(),
  }),
]);

export const FrontalCodeEventSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("turn.started") }),
  z.object({ type: z.literal("item.completed"), item: ThreadItemSchema }),
  z.object({
    type: z.literal("turn.completed"),
    finalResponse: z.string(),
    usage: UsageSchema.optional(),
    sessionId: z.string().optional(),
  }),
  z.object({ type: z.literal("turn.failed"), error: z.string() }),
]);

export type Usage = z.infer<typeof UsageSchema>;
export type ThreadItem = z.infer<typeof ThreadItemSchema>;
export type FrontalCodeEvent = z.infer<typeof FrontalCodeEventSchema>;

/**
 * Parses one line of the event stream. Throws a `ZodError` that names the failing field, so a
 * malformed line is reported with its cause rather than as a generic JSON error.
 */
export function parseFrontalCodeEvent(line: string): FrontalCodeEvent {
  return FrontalCodeEventSchema.parse(JSON.parse(line));
}
