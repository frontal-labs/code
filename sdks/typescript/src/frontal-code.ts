import { defaultCommand } from "./command.js";
import type { FrontalCodeOptions, JsonValue, ThreadOptions } from "./protocol.js";
import { Thread } from "./thread.js";

/**
 * The top-level Frontal Code client. Wraps the Frontal Code CLI (`frontal-code`, or `code`, the same
 * program) and is used to start and resume conversation {@link Thread}s.
 */
export class FrontalCode {
  readonly options: FrontalCodeOptions;
  readonly command: string;

  constructor(options: FrontalCodeOptions = {}) {
    this.options = options;
    this.command = options.command ?? defaultCommand();
  }

  /** Start a fresh conversation thread. */
  startThread(threadOptions: ThreadOptions = {}): Thread {
    return new Thread(this, threadOptions, undefined);
  }

  /** Reconstruct a thread from a previously persisted session id. */
  resumeThread(sessionId: string, threadOptions: ThreadOptions = {}): Thread {
    return new Thread(this, threadOptions, sessionId);
  }

  /**
   * Build the environment for a spawned CLI. Starts from `process.env`, applies
   * the user-provided `env`, then applies any `extra` overrides on top.
   * The `frontal-code` CLI authenticates via provider-specific environment
   * variables (e.g. `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) which the caller
   * is responsible for setting; this SDK does not inject any of its own.
   */
  buildEnv(extra?: Record<string, string>): NodeJS.ProcessEnv {
    const base: NodeJS.ProcessEnv = { ...process.env };
    if (this.options.env) {
      Object.assign(base, this.options.env);
    }
    if (extra) {
      Object.assign(base, extra);
    }
    return base;
  }
}

export type { JsonValue };
