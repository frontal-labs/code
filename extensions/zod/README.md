# @frontal-labs/code-zod-schemas

Zod schemas for the Frontal Code event protocol. The SDKs and extensions share them, so every consumer checks the same contract.

```ts
import { FrontalCodeEventSchema, parseFrontalCodeEvent } from "@frontal-labs/code-zod-schemas";

const event = parseFrontalCodeEvent('{"type":"turn.started"}');
FrontalCodeEventSchema.parse(event);
```

Exports:

- `FrontalCodeEventSchema`: `turn.started`, `item.completed`, `turn.completed`, and `turn.failed`.
- `ThreadItemSchema`, `UsageSchema`: the nested shapes.
- `parseFrontalCodeEvent(line)`: parses one JSONL line and throws a `ZodError` naming the failing field.

The TypeScript SDK's end-to-end suite validates every streamed event from the real CLI against `FrontalCodeEventSchema`.

```bash
cd extensions/zod
bun run test          # vitest
bun run typecheck     # tsc --noEmit
```

This package is private. It is a development dependency of the SDK, not part of the published SDK.
