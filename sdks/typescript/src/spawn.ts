import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import type { FrontalCodeEvent, ThreadItem, Usage } from "./protocol.js";

/** Error raised when the `frontal-code` CLI fails to spawn or exits non-zero. */
export class FrontalCodeCliError extends Error {
  readonly exitCode: number | null;
  constructor(message: string, exitCode: number | null = null) {
    super(message);
    this.name = "FrontalCodeCliError";
    this.exitCode = exitCode;
  }
}

function parseEvent(line: string): FrontalCodeEvent | null {
  const trimmed = line.trim();
  if (!trimmed) return null;
  try {
    const raw = JSON.parse(trimmed) as Partial<FrontalCodeEvent> & {
      type?: string;
    };
    if (!raw || typeof raw.type !== "string") return null;
    return raw as FrontalCodeEvent;
  } catch {
    return null;
  }
}

/**
 * Spawn the CLI and yield parsed {@link FrontalCodeEvent}s as they arrive on stdout.
 * Throws {@link FrontalCodeCliError} if the process exits with a non-zero code.
 */
export async function* streamEvents(
  command: string,
  args: string[],
  env: NodeJS.ProcessEnv,
  cwd?: string,
): AsyncGenerator<FrontalCodeEvent> {
  const child = spawn(command, args, {
    env,
    cwd,
    stdio: ["ignore", "pipe", "pipe"],
  });

  const stdout = child.stdout;
  const stderr = child.stderr;
  if (!stdout || !stderr) {
    throw new FrontalCodeCliError("frontal-code CLI did not provide stdio streams");
  }

  let stderrBuffer = "";
  stderr.on("data", (chunk) => {
    stderrBuffer += chunk.toString();
  });

  const rl = createInterface({ input: stdout });

  try {
    for await (const line of rl) {
      const event = parseEvent(line);
      if (event) yield event;
    }
  } finally {
    const code = await new Promise<number | null>((resolve) => {
      if (child.exitCode !== null && child.exitCode !== undefined) {
        resolve(child.exitCode);
        return;
      }
      child.once("close", (code) => resolve(code));
    });
    if (code !== null && code !== 0) {
      throw new FrontalCodeCliError(
        `frontal-code exited with code ${code}: ${stderrBuffer.trim()}`,
        code,
      );
    }
  }
}

/** The buffered outcome of a turn. */
export interface BufferedTurn {
  events: FrontalCodeEvent[];
  result: {
    finalResponse: string;
    items: ThreadItem[];
    usage?: Usage;
  };
  sessionId?: string;
}

/** Drain a stream of events into a buffered {@link BufferedTurn}. */
export async function collectTurn(
  events: AsyncIterable<FrontalCodeEvent>,
): Promise<BufferedTurn> {
  const collected: FrontalCodeEvent[] = [];
  const items: ThreadItem[] = [];
  let finalResponse = "";
  let usage: Usage | undefined;
  let sessionId: string | undefined;

  for await (const event of events) {
    collected.push(event);
    if (event.type === "item.completed") {
      items.push(event.item);
    } else if (event.type === "turn.completed") {
      finalResponse = event.finalResponse;
      usage = event.usage;
      sessionId = event.sessionId;
    }
  }

  return {
    events: collected,
    result: { finalResponse, items, usage },
    sessionId,
  };
}
