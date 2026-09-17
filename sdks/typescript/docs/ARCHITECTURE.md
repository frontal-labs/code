# TypeScript SDK Architecture

## Overview

The Frontal Code TypeScript SDK (`@frontal-labs/frontal-code-sdk`) is a thin async wrapper
around the `frontal-code` CLI (`@frontal-labs/frontal-code`). It spawns the CLI as a child
process per turn and communicates via JSONL over stdin/stdout using
`--output-format json --stream`.

## High-Level Components

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontal Code Client                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ new Frontal Code() │  │ startThread │  │ resumeThread        │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │             │
│         ▼                ▼                     ▼             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                    Thread                            │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │    │
│  │  │ run()       │  │ runStreamed │  │ id          │   │    │
│  │  └─────────────┘  └─────────────┘  └─────────────┘   │    │
│  └─────────────────────────────────────────────────────┘    │
│         │                │                     │             │
│         ▼                ▼                     ▼             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              ChildProcessManager                      │    │
│  │  - spawns `frontal-code` CLI with --output-format json      │    │
│  │  - manages stdin/stdout/stderr pipes                 │    │
│  │  - parses JSONL event stream                         │    │
│  │  - handles --resume for multi-turn conversations     │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Core Classes

### `Frontal Code` - Main Entry Point

```typescript
class Frontal Code {
  constructor(config?: FrontalCodeConfig);
  
  startThread(options?: ThreadOptions): Thread;
  resumeThread(threadId: string, options?: ThreadOptions): Thread;
}
```

### `Thread` - Conversation Handle

```typescript
class Thread {
  readonly id: string;
  
  run(input: ThreadInput, options?: ThreadRunOptions): Promise<TurnResult>;
  runStreamed(input: ThreadInput, options?: ThreadRunOptions): Promise<StreamedTurn>;
}
```

### `ThreadInput` - Structured Input

```typescript
type ThreadInput = 
  | string                           // Simple text
  | ThreadInputEntry[]               // Structured entries
  | ThreadInputEntry;                // Single entry

interface ThreadInputEntry {
  type: 'text';
  text: string;
}

interface ThreadInputEntry {
  type: 'local_image';
  path: string;
}
```

### `ThreadOptions` - Thread-Level Configuration

```typescript
interface ThreadOptions {
  workingDirectory?: string;
  skipGitRepoCheck?: boolean;
  config?: Record<string, unknown>;  // --config overrides
}
```

### `ThreadRunOptions` - Per-Turn Configuration

```typescript
interface ThreadRunOptions {
  outputSchema?: Record<string, unknown>;  // JSON Schema for structured output
  config?: Record<string, unknown>;        // Per-turn --config overrides
}
```

### `TurnResult` - Buffered Turn Result

```typescript
interface TurnResult {
  finalResponse: string;
  items: Item[];
  usage?: Usage;
}
```

### `StreamedTurn` - Streaming Turn Result

```typescript
interface StreamedTurn {
  events: AsyncGenerator<StreamEvent, void, unknown>;
}
```

### `StreamEvent` - Streaming Events

```typescript
type StreamEvent =
  | { type: 'item_completed'; item: Item }
  | { type: 'turn_completed'; result: TurnResult }
  | { type: 'error'; error: ErrorEvent };
```

## Event Stream Processing

The CLI emits JSONL events on stdout. The SDK parses these into typed events
and yields them via `runStreamed()` as an async generator.

```typescript
const { events } = await thread.runStreamed('Analyze this code');

for await (const event of events) {
  switch (event.type) {
    case 'item_completed':
      console.log('Item:', event.item.type);
      break;
    case 'turn_completed':
      console.log('Done:', event.result.finalResponse);
      break;
    case 'error':
      console.error('Error:', event.error);
      break;
  }
}
```

## Configuration System

Configuration flows through four layers (highest priority last):

1. **CLI defaults** - Built into `frontal-code` binary
2. **Global config** - `~/.frontal-code/config.toml`
3. **Frontal Code constructor config** - `baseUrl`, `env`, `command`
4. **Thread options** - `workingDirectory`, `skipGitRepoCheck`, `config`
5. **Run options** - `outputSchema`, `config` (highest priority)

All config overrides are flattened to dotted paths and passed as repeated
`--config key=value` flags. Values are serialized as TOML literals.

## Testing Architecture

Tests are hermetic - they spawn a mock `frontal-code` script instead of the real CLI:

```typescript
// tests/mock-frontal-code.ts - emits predetermined JSONL events
import { createMockFrontal Code } from './test-utils';

test('thread runs correctly', async () => {
  const frontal-code = createMockFrontal Code({ scenario: 'basic' });
  const thread = frontal-code.startThread();
  const result = await thread.run('Hello');
  
  expect(result.finalResponse).toBe('Mock response');
});
```

This enables fast, offline, deterministic testing without API keys or network.

## Error Handling

All methods throw typed errors:

```typescript
class FrontalCodeError extends Error {
  constructor(
    message: string,
    public readonly code: 'CLI_NOT_FOUND' | 'PROCESS_EXITED' | 
                         'INVALID_EVENT' | 'THREAD_NOT_FOUND' | 'PARSE_ERROR',
    public readonly cause?: Error
  ) {}
}
```

## Requirements

- Node.js 18+
- `frontal-code` CLI on PATH (or custom `command` in config)

## Package Exports

```typescript
// Main entry
import { Frontal Code, Thread, TurnResult, StreamEvent, Item, FrontalCodeConfig, 
         ThreadOptions, ThreadRunOptions, ThreadInput } from '@frontal-labs/frontal-code-sdk';

// FrontalCodeError
import { FrontalCodeError } from '@frontal-labs/frontal-code-sdk/errors';
```