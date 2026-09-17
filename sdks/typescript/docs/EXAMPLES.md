# TypeScript SDK Examples

## Installation

```bash
npm install @frontal-labs/frontal-code-sdk
# Requires Node.js 18+
```

## Basic Usage

### Simple Text Turn

```typescript
import { Frontal Code } from '@frontal-labs/frontal-code-sdk';

const frontal-code = new Frontal Code();
const thread = frontal-code.startThread();

const turn = await thread.run('Explain async/await in TypeScript');
console.log(turn.finalResponse);
```

### Multi-Turn Conversation

```typescript
const thread = frontal-code.startThread();

// Turn 1
const turn1 = await thread.run('Create a React component for a todo list');

// Turn 2 - continues same conversation
const turn2 = await thread.run('Add TypeScript types to the component');

// Access all items from the turn
turn2.items.forEach(item => {
  console.log(`${item.type}: ${item.content ?? JSON.stringify(item.toolCall)}`);
});
```

## Streaming Events

```typescript
const { events } = await thread.runStreamed('Refactor this function for performance');

for await (const event of events) {
  switch (event.type) {
    case 'item_completed':
      console.log(`✓ ${event.item.type}: ${event.item.content?.slice(0, 80)}`);
      break;
    case 'turn_completed':
      console.log('\nFinal:', event.result.finalResponse);
      break;
    case 'error':
      console.error('Error:', event.error);
      break;
  }
}
```

## Structured Output (JSON Schema)

```typescript
const schema = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    issues: { 
      type: 'array', 
      items: { type: 'string' } 
    },
    severity: { 
      type: 'string', 
      enum: ['low', 'medium', 'high', 'critical'] 
    }
  },
  required: ['summary', 'severity'],
  additionalProperties: false
};

const result = await thread.run('Analyze this code for security issues', {
  outputSchema: schema
});

// result.finalResponse is guaranteed valid JSON matching schema
const analysis = JSON.parse(result.finalResponse);
console.log(`Severity: ${analysis.severity}`);
analysis.issues.forEach(issue => console.log(`- ${issue}`));
```

## Image Input

```typescript
const result = await thread.run([
  { type: 'text', text: 'Describe the UI changes between these screenshots' },
  { type: 'local_image', path: './before.png' },
  { type: 'local_image', path: './after.png' }
]);
```

## Configuration

### Custom CLI Path

```typescript
const frontal-code = new Frontal Code({
  command: '/custom/path/to/frontal-code'
});
```

### Custom API Base URL

```typescript
const frontal-code = new Frontal Code({
  baseUrl: 'https://api.example.com'  // maps to frontal_base_url
});
```

### Environment Variables

```typescript
const frontal-code = new Frontal Code({
  env: {
    CUSTOM_VAR: 'value',
    DEBUG: 'frontal-code:*'
  }
});
```

### Global Config Overrides

```typescript
const frontal-code = new Frontal Code({
  config: {
    show_raw_agent_reasoning: true,
    sandbox_workspace_write: { network_access: true }
  }
});
```

### Per-Turn Config Overrides

```typescript
const result = await thread.run('Be concise', {
  config: {
    model: 'gpt-4',
    temperature: 0.1
  }
});
```

## Thread Options

### Working Directory

```typescript
const thread = frontal-code.startThread({
  workingDirectory: '/path/to/project'
});
```

### Skip Git Repo Check

```typescript
const thread = frontal-code.startThread({
  workingDirectory: '/non-git/directory',
  skipGitRepoCheck: true
});
```

## Resume Existing Thread

```typescript
// First run - persist the thread ID
const thread = frontal-code.startThread();
const threadId = thread.id;
await saveThreadId(threadId); // Your persistence logic

// Later - resume the conversation
const resumedThread = frontal-code.resumeThread(threadId);
const turn = await resumedThread.run('Continue from where we left off');
```

## Error Handling

```typescript
import { FrontalCodeError } from '@frontal-labs/frontal-code-sdk/errors';

try {
  const turn = await thread.run('Analyze this code');
} catch (error) {
  if (error instanceof FrontalCodeError) {
    switch (error.code) {
      case 'CLI_NOT_FOUND':
        console.error('Frontal Code CLI not installed. Run: npm i -g @frontal-labs/frontal-code');
        break;
      case 'PROCESS_EXITED':
        console.error('CLI crashed:', error.message);
        break;
      case 'THREAD_NOT_FOUND':
        console.error('Session expired or not found');
        break;
      case 'INVALID_EVENT':
        console.error('Protocol error:', error.message);
        break;
    }
  } else {
    console.error('Unexpected error:', error);
  }
}
```

## TypeScript Configuration

For best type safety with structured output:

```typescript
// types/security-analysis.ts
export interface SecurityAnalysis {
  summary: string;
  issues: string[];
  severity: 'low' | 'medium' | 'high' | 'critical';
}

// Usage
const schema = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    issues: { type: 'array', items: { type: 'string' } },
    severity: { type: 'string', enum: ['low', 'medium', 'high', 'critical'] }
  },
  required: ['summary', 'severity'],
  additionalProperties: false
};

const result = await thread.run('Check for SQL injection', { outputSchema: schema });
const analysis: SecurityAnalysis = JSON.parse(result.finalResponse);
```

## Advanced: Custom Event Processing

```typescript
async function processWithProgress(thread: Thread, input: string) {
  const { events } = await thread.runStreamed(input);
  
  const items: Item[] = [];
  
  for await (const event of events) {
    if (event.type === 'item_completed') {
      items.push(event.item);
      // Real-time UI updates
      updateProgress(event.item);
    } else if (event.type === 'turn_completed') {
      return { items, finalResponse: event.result.finalResponse };
    }
  }
}
```

## Testing with Mock CLI

```typescript
import { createMockFrontal Code } from '@frontal-labs/frontal-code-sdk/test-utils';

test('handles streaming correctly', async () => {
  const frontal-code = createMockFrontal Code({ scenario: 'streaming_tool_calls' });
  const thread = frontal-code.startThread();
  
  const { events } = await thread.runStreamed('Run tests');
  
  const toolCalls: Item[] = [];
  for await (const event of events) {
    if (event.type === 'item_completed' && event.item.type === 'tool_call') {
      toolCalls.push(event.item);
    }
  }
  
  expect(toolCalls.length).toBeGreaterThan(0);
});
```