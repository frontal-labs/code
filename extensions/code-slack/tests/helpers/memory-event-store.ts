import { type EventEnvelope, eventType } from "@openchannelprotocol/sdk";
import type { EventStore, OcpStoreHandle } from "../../src/store";

type ActionRecord = NonNullable<Awaited<ReturnType<NonNullable<EventStore["getAction"]>>>>;

// In-memory EventStore with the same semantics the runtime relies on from the Postgres store:
// idempotent appends keyed by eventId and single-claim action records.
export class MemoryEventStore implements EventStore {
  readonly events: EventEnvelope[] = [];
  readonly actions = new Map<string, ActionRecord>();

  async all(): Promise<EventEnvelope[]> {
    return [...this.events];
  }

  async append(event: EventEnvelope): Promise<void> {
    await this.appendIfAbsent(event);
  }

  async appendIfAbsent(event: EventEnvelope): Promise<boolean> {
    if (this.events.some((stored) => stored.eventId === event.eventId)) {
      return false;
    }
    this.events.push(event);
    return true;
  }

  async has(eventId: string): Promise<boolean> {
    return this.events.some((event) => event.eventId === eventId);
  }

  async get(eventId: string): Promise<EventEnvelope | undefined> {
    return this.events.find((event) => event.eventId === eventId);
  }

  async list(options: { eventType?: string; limit?: number } = {}): Promise<EventEnvelope[]> {
    const matching = options.eventType
      ? this.events.filter((event) => eventType(event) === options.eventType)
      : this.events;
    return matching.slice(0, options.limit ?? matching.length);
  }

  query(options?: { eventType?: string; limit?: number }): Promise<EventEnvelope[]> {
    return this.list(options);
  }

  async claimAction(actionId: string, owner: string): Promise<boolean> {
    if (this.actions.has(actionId)) {
      return false;
    }
    this.actions.set(actionId, { actionId, owner, status: "claimed" });
    return true;
  }

  async completeAction(actionId: string, result: unknown): Promise<void> {
    this.actions.set(actionId, { actionId, result, status: "completed" });
  }

  async failAction(actionId: string, error: unknown): Promise<void> {
    this.actions.set(actionId, { actionId, error, status: "failed" });
  }

  async getAction(actionId: string): Promise<ActionRecord | undefined> {
    return this.actions.get(actionId);
  }

  eventTypes(): string[] {
    return this.events.map((event) => eventType(event));
  }
}

export function memoryStoreHandle(store = new MemoryEventStore()): OcpStoreHandle & {
  store: MemoryEventStore;
} {
  return {
    store,
    migrate: async () => undefined,
    ping: async () => true,
    close: async () => undefined,
  };
}
