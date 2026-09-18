import { beforeEach, describe, expect, it, vi } from "vitest";

const { MockWebSocket, wsInstances } = vi.hoisted(() => {
  const wsInstances: Array<InstanceType<typeof MockWebSocket>> = [];

  class MockWebSocket {
    static OPEN = 1;

    public readonly url: string;
    public readyState = 0;
    private readonly handlers = new Map<
      string,
      Array<(...args: unknown[]) => void>
    >();

    constructor(url: string) {
      this.url = url;
      wsInstances.push(this);
    }

    on(event: string, handler: (...args: unknown[]) => void): void {
      const handlers = this.handlers.get(event) ?? [];
      handlers.push(handler);
      this.handlers.set(event, handlers);
    }

    close(): void {
      this.readyState = 3;
    }

    emit(event: string, ...args: unknown[]): void {
      const handlers = this.handlers.get(event) ?? [];
      for (const handler of handlers) {
        handler(...args);
      }
    }
  }

  return { MockWebSocket, wsInstances };
});

vi.mock("ws", () => ({
  default: MockWebSocket,
}));

vi.mock("../src/config", () => ({
  config: {
    app: {
      nodeEnv: "test",
      logLevel: "error",
      port: 3000,
    },
    frontal_code: {
      apiUrl: "https://frontal-code.example.com/",
      timeout: 30_000,
    },
  },
}));

import { config } from "../src/config";
import { FrontalCodeEventsClient } from "../src/events-client";
import { logger } from "../src/log";

type TestableLegacyFrontalCodeEventsClient = FrontalCodeEventsClient & {
  buildWsUrl(): string;
  ws?: { readyState: number; close(): void };
};

describe("Legacy FrontalCodeEventsClient", () => {
  beforeEach(() => {
    wsInstances.length = 0;
    vi.useRealTimers();
  });

  it("builds a websocket URL from the hosted API URL", () => {
    const client =
      new FrontalCodeEventsClient() as TestableLegacyFrontalCodeEventsClient;

    expect(client.buildWsUrl()).toBe(
      "wss://frontal-code.example.com/v1/events/ws"
    );
  });

  it("builds a ws URL from a non-secure hosted API URL", async () => {
    vi.resetModules();
    vi.doMock("../src/config", () => ({
      config: {
        app: {
          nodeEnv: "test",
          logLevel: "error",
          port: 3000,
        },
        frontal_code: {
          apiUrl: "http://frontal-code.example.com/",
          timeout: 30_000,
        },
      },
    }));

    const { FrontalCodeEventsClient: HttpFrontalCodeEventsClient } =
      await import("../src/events-client");
    const client =
      new HttpFrontalCodeEventsClient() as TestableLegacyFrontalCodeEventsClient;

    expect(client.buildWsUrl()).toBe(
      "ws://frontal-code.example.com/v1/events/ws"
    );

    vi.doMock("../src/config", () => ({
      config: {
        app: {
          nodeEnv: "test",
          logLevel: "error",
          port: 3000,
        },
        frontal_code: {
          apiUrl: "https://frontal-code.example.com/",
          timeout: 30_000,
        },
      },
    }));
  });

  it("builds a ws URL from an http API base by mutating the shared config object", () => {
    const client =
      new FrontalCodeEventsClient() as TestableLegacyFrontalCodeEventsClient;
    const originalUrl = config.frontal - code.apiUrl;
    config.frontal-code.apiUrl = "http://frontal-code.example.com/";

    try {
      expect(client.buildWsUrl()).toBe(
        "ws://frontal-code.example.com/v1/events/ws"
      );
    } finally {
      config.frontal-code.apiUrl = originalUrl;
    }
  });

  it("leaves websocket-native API URLs unchanged when building the event stream URL", async () => {
    vi.resetModules();
    vi.doMock("../src/config", () => ({
      config: {
        app: {
          nodeEnv: "test",
          logLevel: "error",
          port: 3000,
        },
        frontal_code: {
          apiUrl: "ws://frontal-code.example.com/",
          timeout: 30_000,
        },
      },
    }));

    const { FrontalCodeEventsClient: WsFrontalCodeEventsClient } = await import(
      "../src/events-client"
    );
    const client =
      new WsFrontalCodeEventsClient() as TestableLegacyFrontalCodeEventsClient;

    expect(client.buildWsUrl()).toBe(
      "ws://frontal-code.example.com/v1/events/ws"
    );

    vi.doMock("../src/config", () => ({
      config: {
        app: {
          nodeEnv: "test",
          logLevel: "error",
          port: 3000,
        },
        frontal_code: {
          apiUrl: "https://frontal-code.example.com/",
          timeout: 30_000,
        },
      },
    }));
  });

  it("dispatches parsed event payloads to registered handlers", () => {
    const client = new FrontalCodeEventsClient();
    const handler = vi.fn();
    const infoSpy = vi.spyOn(logger, "info").mockImplementation(() => {});
    client.onEvent(handler);
    client.connect();

    const socket = wsInstances[0];
    socket.emit("open");
    socket.emit(
      "message",
      Buffer.from(
        JSON.stringify({
          event_id: "evt-123",
          topic: "lane",
          event: "lane.started",
          status: "running",
          emittedAt: "2026-04-09T10:00:00Z",
          task_id: "task-123",
        })
      )
    );

    expect(handler).toHaveBeenCalledWith(
      expect.objectContaining({
        event: "lane.started",
        task_id: "task-123",
      })
    );
    expect(infoSpy).toHaveBeenCalledWith(
      "Connected to Frontal Code event stream",
      {
        url: "wss://frontal-code.example.com/v1/events/ws",
      }
    );
  });

  it("logs malformed event payloads without dispatching handlers", () => {
    const errorSpy = vi.spyOn(logger, "error").mockImplementation(() => {});
    const client = new FrontalCodeEventsClient();
    const handler = vi.fn();
    client.onEvent(handler);
    client.connect();

    const socket = wsInstances[0];
    socket.emit("message", Buffer.from("not-json"));

    expect(handler).not.toHaveBeenCalled();
    expect(errorSpy).toHaveBeenCalledWith(
      "Failed to parse Frontal Code event",
      expect.any(Error)
    );
  });

  it("does not reconnect when an open socket already exists", () => {
    const client =
      new FrontalCodeEventsClient() as TestableLegacyFrontalCodeEventsClient;
    client.ws = {
      readyState: MockWebSocket.OPEN,
      close: vi.fn(),
    };

    client.connect();

    expect(wsInstances).toHaveLength(0);
  });

  it("schedules a reconnect after the socket closes", () => {
    vi.useFakeTimers();
    const client = new FrontalCodeEventsClient();
    client.connect();

    const firstSocket = wsInstances[0];
    firstSocket.emit("close", 1006, Buffer.from("disconnected"));

    expect(wsInstances).toHaveLength(1);
    vi.advanceTimersByTime(2_000);
    expect(wsInstances).toHaveLength(2);
  });

  it("does not schedule duplicate reconnects for close and error before the timer fires", () => {
    vi.useFakeTimers();
    const client = new FrontalCodeEventsClient();
    client.connect();

    const firstSocket = wsInstances[0];
    firstSocket.emit("close", 1006, Buffer.from("disconnected"));
    firstSocket.emit("error", new Error("socket failed"));

    vi.advanceTimersByTime(2_000);

    expect(wsInstances).toHaveLength(2);
  });

  it("cancels a pending reconnect when the client is closed", () => {
    vi.useFakeTimers();
    const client =
      new FrontalCodeEventsClient() as TestableLegacyFrontalCodeEventsClient;
    client.connect();

    const firstSocket = wsInstances[0];
    firstSocket.emit("close", 1006, Buffer.from("disconnected"));
    client.close();
    vi.advanceTimersByTime(2_000);

    expect(firstSocket.readyState).toBe(3);
    expect(wsInstances).toHaveLength(1);
  });
});
