# TypeScript SDK Testing Guide

## Test Architecture

Tests are fully hermetic - they do **not** require:
- Network access
- API keys (`CODEX_API_KEY`)
- Real `frontal-code` CLI binary

Instead, tests use a mock CLI (`tests/mock-frontal-code.ts`) that emits
predefined JSONL events over stdout.

## Running Tests

CI installs with Bun from the repository root. Run the same commands locally:

```bash
# From the repository root
bun install
bun run --filter @frontal-labs/code-sdk test

# One file (from sdks/typescript)
bunx vitest run tests/thread.test.ts

# Watch mode (from sdks/typescript)
bunx vitest
```

`npm ci` inside `sdks/typescript` does not work: the root `package.json` declares the workspace, and the lockfile that CI uses is `bun.lock`.

### End-to-end suite (real CLI)

`tests/e2e.test.ts` runs the real `code` binary against a live model. It is skipped unless `FRONTAL_CODE_E2E_BIN` is set. The default provider is a local Ollama server:

```bash
cargo build -p cli
ollama pull qwen2.5:1.5b
FRONTAL_CODE_E2E_BIN="$PWD/target/debug/code" \
  npx vitest run tests/e2e.test.ts
```

Every streamed event is checked against the shared schema in `extensions/zod`.

Optional: `FRONTAL_CODE_E2E_PROVIDER` (default `ollama`; `openai` also needs `OPENAI_API_KEY`; `anthropic` uses `FRONTAL_BASE_URL` and `FRONTAL_API_KEY`, as in `scripts/verify-claude-gateway.sh`), `FRONTAL_CODE_E2E_MODEL` (default `qwen2.5:1.5b`), `OLLAMA_BASE_URL` (default `http://localhost:11434`), and `FRONTAL_CODE_E2E_MAX_TOKENS` (default `1024`). A skipped run reports as passing, so check that the variables are set before you read a green result.

## Test Structure

```
tests/
├── mock-frontal-code.ts           # Mock CLI - emits JSONL events
├── test-utils.ts           # Shared test utilities
├── thread.test.ts          # Basic thread run tests
├── streaming.test.ts       # Stream event tests
├── structured-output.test.ts # JSON Schema / structured output tests
├── resume.test.ts          # Thread resume tests
├── config.test.ts          # Config override tests
├── images.test.ts          # Image input tests
└── errors.test.ts          # Error handling tests
```

## Mock CLI Protocol

The mock CLI (`tests/mock-frontal-code.ts`) reads stdin for the request payload
and writes JSONL events to stdout.

### Request Format

```typescript
interface MockRequest {
  input: string;
  session_id?: string;
  config?: Record<string, unknown>;
  output_schema?: Record<string, unknown>;
}
```

### Response Format (JSONL)

```jsonl
{"type":"item_completed","item":{"id":"msg-1","type":"message","content":"Hello!"}}
{"type":"turn_completed","result":{"final_response":"Hello!","items":[],"usage":{}}}
```

### Adding New Test Scenarios

Edit `tests/mock-frontal-code.ts`:

```typescript
const SCENARIOS: Record<string, string[]> = {
  // Existing scenarios...
  
  my_new_scenario: [
    '{"type":"item_completed","item":{"id":"msg-1","type":"message","content":"Custom response"}}',
    '{"type":"turn_completed","result":{"final_response":"Custom response","items":[],"usage":{}}}'
  ],
};
```

Then use in tests:

```typescript
import { createMockFrontal Code } from './test-utils';

test('my new scenario', async () => {
  const frontal-code = createMockFrontal Code({ scenario: 'my_new_scenario' });
  const thread = frontal-code.startThread();
  const result = await thread.run('test input');
  
  expect(result.finalResponse).toBe('Custom response');
});
```

## Test Utilities

### `createMockFrontal Code(options)`

```typescript
const frontal-code = createMockFrontal Code({
  scenario: 'basic',           // scenario name from mock-frontal-code.ts
  env: { CUSTOM_VAR: 'value' }, // extra env vars
  cwd: '/custom/dir'           // working directory
});
```

### `assertEventSequence(events, ...expectedTypes)`

```typescript
import { assertEventSequence } from './test-utils';

const { events } = await thread.runStreamed('input');
assertEventSequence(events, 'item_completed', 'turn_completed');
```

### `collectEvents(events)`

```typescript
import { collectEvents } from './test-utils';

const { events } = await thread.runStreamed('input');
const allEvents = await collectEvents(events);

expect(allEvents.filter(e => e.type === 'item_completed')).toHaveLength(3);
```

## Writing New Tests

### Basic Pattern

```typescript
import { createMockFrontal Code } from './test-utils';
import { FrontalCodeError } from '../src/errors';

test('descriptive test name', async () => {
  // Arrange
  const frontal-code = createMockFrontal Code({ scenario: 'my_scenario' });
  const thread = frontal-code.startThread();
  
  // Act
  const result = await thread.run('test input');
  
  // Assert
  expect(result.finalResponse).toBe('expected output');
  expect(result.items).toHaveLength(1);
});
```

### Streaming Test

```typescript
test('streams tool calls in real-time', async () => {
  const frontal-code = createMockFrontal Code({ scenario: 'streaming_tool_calls' });
  const thread = frontal-code.startThread();
  
  const { events } = await thread.runStreamed('run tests');
  
  const toolCalls: Item[] = [];
  for await (const event of events) {
    if (event.type === 'item_completed' && event.item.type === 'tool_call') {
      toolCalls.push(event.item);
    }
  }
  
  expect(toolCalls.length).toBeGreaterThan(0);
});
```

### Error Test

```typescript
test('throws FrontalCodeError when CLI not found', async () => {
  const frontal-code = new Frontal Code({ command: '/nonexistent/frontal-code' });
  const thread = frontal-code.startThread();
  
  await expect(thread.run('test')).rejects.toThrow(FrontalCodeError);
  await expect(thread.run('test')).rejects.toMatchObject({
    code: 'CLI_NOT_FOUND'
  });
});
```

### Config Override Test

```typescript
test('passes config overrides to CLI', async () => {
  const frontal-code = createMockFrontal Code({ scenario: 'echo_config' });
  const thread = frontal-code.startThread();
  
  await thread.run('test', {
    config: { temperature: 0.5, model: 'gpt-4' }
  });
  
  // mock-frontal-code echoes received config in response
  // assert config was passed correctly
});
```

## Mock Scenario Reference

| Scenario | Description |
|----------|-------------|
| `basic` | Simple text response |
| `streaming` | Multiple item_completed events |
| `streaming_tool_calls` | Tool call events in stream |
| `structured_output` | Valid JSON matching schema |
| `resume_thread` | Session resume flow |
| `image_input` | Image input handling |
| `config_echo` | Echoes received config |
| `error_cli_exit` | CLI process exits with error |
| `error_invalid_json` | Malformed JSONL from CLI |

## Continuous Integration

Tests run in GitHub Actions on every PR:

```yaml
# .github/workflows/typescript-sdk.yml
- name: Run tests
  run: npm test
  
- name: Check coverage
  run: npm run test:coverage
```

## Coverage Goals

- Statements: >90%
- Branches: >85%
- Functions: >90%
- Lines: >90%

Run `npm run test:coverage` and open `coverage/lcov-report/index.html` for details.