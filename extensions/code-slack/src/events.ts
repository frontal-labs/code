// Event-level routing for Frontal Code task events, independent of how they arrive. Owns task
// routing hints, the per-task replay buffer, duplicate suppression, and handler fan-out.
// The WebSocket transport lives in `websocket.ts` and feeds events in through `receive`.
import { logger } from "./log";
import type { FrontalCodeEventEnvelope, FrontalCodeTrackedTask } from "./types";

export type TrackedTaskHandler = (
  event: FrontalCodeEventEnvelope,
  task?: FrontalCodeTrackedTask,
) => Promise<void> | void;

export const RECENT_EVENTS_PER_TASK = 12;
export const MAX_SEEN_EVENT_KEYS = 512;

export class FrontalCodeEventRouter {
  readonly taskHints = new Map<string, FrontalCodeTrackedTask>();
  readonly recentEvents = new Map<string, FrontalCodeEventEnvelope[]>();
  private readonly handlers = new Set<TrackedTaskHandler>();
  private readonly seenEventKeys = new Set<string>();
  private readonly seenEventOrder: string[] = [];

  onTrackedTaskEvent(handler: TrackedTaskHandler): () => void {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }

  // Registers routing for a task and replays its buffered events to the handlers.
  trackTask(task: FrontalCodeTrackedTask): void {
    this.taskHints.set(task.taskId, task);
    const recentEvents = this.recentEvents.get(task.taskId) ?? [];
    for (const event of recentEvents) {
      this.dispatchEvent(event, task);
    }
  }

  untrackTask(taskId: string): void {
    this.taskHints.delete(taskId);
    this.recentEvents.delete(taskId);
  }

  // Buffers an incoming event for its task, then dispatches it. Events without a task are ignored.
  receive(event: FrontalCodeEventEnvelope): void {
    if (!event.task_id) {
      return;
    }

    const recent = this.recentEvents.get(event.task_id) ?? [];
    recent.push(event);
    if (recent.length > RECENT_EVENTS_PER_TASK) {
      recent.shift();
    }
    this.recentEvents.set(event.task_id, recent);

    this.dispatchEvent(event);
  }

  dispatchEvent(event: FrontalCodeEventEnvelope, hintedTask?: FrontalCodeTrackedTask): void {
    const taskId = event.task_id;
    if (!taskId) {
      return;
    }

    const task = hintedTask || this.taskHints.get(taskId) || readTrackedTaskFromEvent(event);
    if (task && !this.taskHints.has(taskId)) {
      this.taskHints.set(taskId, task);
    }

    const eventKey = eventDedupeKey(event);
    if (this.seenEventKeys.has(eventKey)) {
      return;
    }

    this.seenEventKeys.add(eventKey);
    this.seenEventOrder.push(eventKey);
    if (this.seenEventOrder.length > MAX_SEEN_EVENT_KEYS) {
      const oldest = this.seenEventOrder.shift();
      if (oldest) {
        this.seenEventKeys.delete(oldest);
      }
    }

    for (const handler of this.handlers) {
      void Promise.resolve(handler(event, task)).catch((error) => {
        logger.error("Frontal Code hosted event handler failed", error as Error, {
          event: event.event,
          taskId,
        });
      });
    }
  }
}

function eventDedupeKey(event: FrontalCodeEventEnvelope): string {
  return `${event.event}:${event.status}:${event.emittedAt}:${event.task_id ?? ""}:${event.lane_id ?? ""}`;
}

// Derives Slack routing from the event payload when no task was tracked yet.
export function readTrackedTaskFromEvent(
  event: FrontalCodeEventEnvelope,
): FrontalCodeTrackedTask | undefined {
  const payload = event.payload;
  if (!payload) {
    return undefined;
  }

  const channelId = payload.channel_id;
  if (!channelId || !event.task_id) {
    return undefined;
  }

  return {
    taskId: event.task_id,
    channelId,
    threadTs: payload.thread_ts,
    userId: payload.user_id,
  };
}
