import { beforeEach, describe, expect, it, vi } from "vitest";

const slackState = vi.hoisted(() => {
  let messageHandler: ((args: { message: unknown }) => Promise<void>) | undefined;
  let commandHandler:
    | ((args: {
        command: { text: string; user_id: string; channel_id: string };
        ack: (arg?: unknown) => Promise<void> | void;
      }) => Promise<void>)
    | undefined;
  let actionHandler:
    | ((args: { action: unknown; ack: () => Promise<void> | void; body: unknown }) => Promise<void>)
    | undefined;
  // Event listeners are keyed by their pattern so tests can reach both app_mention and the
  // catch-all connector forwarder.
  const eventHandlers = new Map<
    string,
    (args: { event: unknown; next: () => Promise<void> }) => Promise<void>
  >();

  const chatPostMessage = vi.fn();
  const chatUpdate = vi.fn();
  const chatPostEphemeral = vi.fn();
  const appStart = vi.fn();
  const appStop = vi.fn();
  const isListening = vi.fn();

  class MockApp {
    public readonly client = {
      chat: {
        postMessage: chatPostMessage,
        update: chatUpdate,
        postEphemeral: chatPostEphemeral,
      },
    };

    start = appStart;
    stop = appStop;
    isListening = isListening;

    message(handler: typeof messageHandler): void {
      messageHandler = handler;
    }

    command(_name: string, handler: typeof commandHandler): void {
      commandHandler = handler;
    }

    action(_pattern: unknown, handler: typeof actionHandler): void {
      actionHandler = handler;
    }

    event(
      pattern: unknown,
      handler: (args: { event: unknown; next: () => Promise<void> }) => Promise<void>,
    ): void {
      eventHandlers.set(String(pattern), handler);
    }
  }

  return {
    MockApp,
    chatPostMessage,
    chatUpdate,
    chatPostEphemeral,
    appStart,
    appStop,
    isListening,
    getMessageHandler: () => messageHandler,
    getCommandHandler: () => commandHandler,
    getActionHandler: () => actionHandler,
    getEventHandler: (pattern = "/.*/") => eventHandlers.get(pattern),
    resetHandlers: () => {
      messageHandler = undefined;
      commandHandler = undefined;
      actionHandler = undefined;
      eventHandlers.clear();
    },
  };
});

const frontalCodeApiState = vi.hoisted(() => {
  const createTask = vi.fn();
  const getOrphanPolicy = vi.fn();
  const sendConnectorEvent = vi.fn();
  const getEventsWebSocketUrl = vi
    .fn()
    .mockReturnValue("ws://localhost:8787/v1/events/ws?source=slack");

  class MockFrontalCodeApiClient {
    createTask = createTask;
    getOrphanPolicy = getOrphanPolicy;
    sendConnectorEvent = sendConnectorEvent;
    getEventsWebSocketUrl = getEventsWebSocketUrl;
  }

  return {
    MockFrontalCodeApiClient,
    createTask,
    getOrphanPolicy,
    sendConnectorEvent,
    getEventsWebSocketUrl,
  };
});

const frontalCodeEventsState = vi.hoisted(() => {
  let trackedHandler: ((event: unknown, task: unknown) => Promise<void> | void) | undefined;

  const connect = vi.fn();
  const disconnect = vi.fn();
  const trackTask = vi.fn();
  const untrackTask = vi.fn();
  const onTrackedTaskEvent = vi.fn((handler: typeof trackedHandler) => {
    trackedHandler = handler;
    return () => undefined;
  });

  class MockFrontalCodeEventsClient {
    connect = connect;
    disconnect = disconnect;
    trackTask = trackTask;
    untrackTask = untrackTask;
    onTrackedTaskEvent = onTrackedTaskEvent;
  }

  return {
    MockFrontalCodeEventsClient,
    connect,
    disconnect,
    trackTask,
    untrackTask,
    onTrackedTaskEvent,
    getTrackedHandler: () => trackedHandler,
    resetTrackedHandler: () => {
      trackedHandler = undefined;
    },
  };
});

// Mutable allowlist read by the mocked config each time a SlackInterface is constructed.
const accessState = vi.hoisted(() => ({
  allowedUserIds: [] as string[],
  allowedChannelIds: [] as string[],
  adminUserIds: [] as string[],
}));

// Each SlackInterface gets a fresh in-memory OCP event store instead of Postgres.
vi.mock("../src/store", async () => {
  const { memoryStoreHandle } = await import("./helpers/memory-event-store");
  return { createOcpStore: () => memoryStoreHandle() };
});

vi.mock("@slack/bolt", () => ({
  App: slackState.MockApp,
}));

vi.mock("../src/api-client", () => ({
  FrontalCodeApiClient: frontalCodeApiState.MockFrontalCodeApiClient,
}));

vi.mock("../src/websocket", () => ({
  FrontalCodeEventsClient: frontalCodeEventsState.MockFrontalCodeEventsClient,
}));

vi.mock("../src/config", () => ({
  config: {
    app: {
      nodeEnv: "test",
      logLevel: "error",
      port: 3000,
    },
    slack: {
      botToken: "xoxb-test",
      appToken: "xapp-test",
      signingSecret: "secret",
      get allowedUserIds() {
        return accessState.allowedUserIds;
      },
      get allowedChannelIds() {
        return accessState.allowedChannelIds;
      },
      get adminUserIds() {
        return accessState.adminUserIds;
      },
    },
    frontalCode: {
      apiUrl: "http://localhost:8787",
      timeout: 30_000,
    },
    ocp: {
      databaseUrl: "postgres://ocp:ocp@localhost:5432/ocp_test",
      eventsTable: "ocp_events",
      runMigrations: true,
      agentId: "agent:frontal-code",
    },
  },
}));

import { SlackInterface } from "../src/slack";

describe("SlackInterface constructor wiring", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    slackState.resetHandlers();
    frontalCodeEventsState.resetTrackedHandler();
    accessState.allowedUserIds = ["U123"];
    accessState.allowedChannelIds = [];
    accessState.adminUserIds = [];
  });

  it("constructs the Slack app, Frontal Code clients, and tracked-event subscription", async () => {
    const handleSpy = vi
      .spyOn(
        SlackInterface.prototype as unknown as {
          handleFrontalCodeTaskEvent: (...args: unknown[]) => Promise<void>;
        },
        "handleFrontalCodeTaskEvent",
      )
      .mockResolvedValue(undefined);
    const _slack = new SlackInterface();

    expect(frontalCodeApiState.getEventsWebSocketUrl).not.toHaveBeenCalled();
    expect(frontalCodeEventsState.onTrackedTaskEvent).toHaveBeenCalledTimes(1);
    expect(slackState.getMessageHandler()).toBeTypeOf("function");
    expect(slackState.getCommandHandler()).toBeTypeOf("function");
    expect(slackState.getActionHandler()).toBeTypeOf("function");
    expect(slackState.getEventHandler()).toBeTypeOf("function");
    expect(slackState.getEventHandler("app_mention")).toBeTypeOf("function");

    await frontalCodeEventsState.getTrackedHandler()?.(
      { event: "lane.started" },
      { taskId: "task-123" },
    );

    expect(handleSpy).toHaveBeenCalledWith({ event: "lane.started" }, { taskId: "task-123" });
  });

  it("ignores Slack message events without a user-facing prompt", async () => {
    new SlackInterface();

    await slackState.getMessageHandler()?.({
      message: {
        text: "",
        user: "U123",
        channel: "D123",
        ts: "1710000000.100",
        channel_type: "im",
      },
    });
    await slackState.getMessageHandler()?.({
      message: {
        text: "hello",
        user: "U123",
        channel: "D123",
        ts: "1710000000.101",
        bot_id: "B123",
        channel_type: "im",
      },
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("ignores ordinary channel messages even from allowlisted users (no mention)", async () => {
    new SlackInterface();

    await slackState.getMessageHandler()?.({
      message: {
        text: "anyone want to look at the flaky test?",
        user: "U123",
        channel: "C123",
        ts: "1710000000.102",
        channel_type: "channel",
      },
    });
    await slackState.getMessageHandler()?.({
      message: {
        text: "reply in a thread",
        user: "U123",
        channel: "C123",
        ts: "1710000000.103",
        thread_ts: "1710000000.100",
        channel_type: "group",
      },
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("creates and registers a task from a direct message by an allowlisted user", async () => {
    const registerSpy = vi
      .spyOn(
        SlackInterface.prototype as unknown as {
          registerTask: (...args: unknown[]) => Promise<void>;
        },
        "registerTask",
      )
      .mockResolvedValue(undefined);
    frontalCodeApiState.createTask.mockResolvedValue({
      task_id: "task-123",
      status: "running",
      message: "created",
    });
    new SlackInterface();

    await slackState.getMessageHandler()?.({
      message: {
        text: "Investigate flaky test",
        user: "U123",
        channel: "D123",
        ts: "1710000000.100",
        channel_type: "im",
      },
    });

    expect(frontalCodeApiState.createTask).toHaveBeenCalledWith({
      prompt: "Investigate flaky test",
      user_id: "U123",
      channel_id: "D123",
      thread_ts: "1710000000.100",
      source: "slack",
    });
    expect(registerSpy).toHaveBeenCalledWith(
      expect.objectContaining({ task_id: "task-123" }),
      {
        taskId: "task-123",
        channelId: "D123",
        threadTs: "1710000000.100",
        userId: "U123",
      },
      "Investigate flaky test",
    );
  });

  it("denies direct messages from users who are not allowlisted", async () => {
    new SlackInterface();

    await slackState.getMessageHandler()?.({
      message: {
        text: "run something expensive",
        user: "U999",
        channel: "D999",
        ts: "1710000000.104",
        channel_type: "im",
      },
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("denies direct messages when the user allowlist is empty", async () => {
    accessState.allowedUserIds = [];
    new SlackInterface();

    await slackState.getMessageHandler()?.({
      message: {
        text: "hello",
        user: "U123",
        channel: "D123",
        ts: "1710000000.105",
        channel_type: "im",
      },
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("starts a task when the bot is mentioned in an allowlisted channel, stripping the mention", async () => {
    const registerSpy = vi
      .spyOn(
        SlackInterface.prototype as unknown as {
          registerTask: (...args: unknown[]) => Promise<void>;
        },
        "registerTask",
      )
      .mockResolvedValue(undefined);
    accessState.allowedChannelIds = ["C123"];
    frontalCodeApiState.createTask.mockResolvedValue({
      task_id: "task-mention",
      status: "running",
      message: "created",
    });
    const next = vi.fn().mockResolvedValue(undefined);
    new SlackInterface();

    await slackState.getEventHandler("app_mention")?.({
      event: {
        type: "app_mention",
        user: "U123",
        channel: "C123",
        ts: "1710000000.200",
        text: "<@UBOT123> fix the flaky test",
      },
      next,
    });

    expect(frontalCodeApiState.createTask).toHaveBeenCalledWith({
      prompt: "fix the flaky test",
      user_id: "U123",
      channel_id: "C123",
      thread_ts: "1710000000.200",
      source: "slack",
    });
    expect(registerSpy).toHaveBeenCalledTimes(1);
    expect(next).toHaveBeenCalled();
  });

  it("ignores a mention from a user who is not allowlisted", async () => {
    accessState.allowedChannelIds = ["C123"];
    const next = vi.fn().mockResolvedValue(undefined);
    new SlackInterface();

    await slackState.getEventHandler("app_mention")?.({
      event: {
        type: "app_mention",
        user: "U999",
        channel: "C123",
        ts: "1710000000.201",
        text: "<@UBOT123> run a task",
      },
      next,
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
    expect(next).toHaveBeenCalled();
  });

  it("ignores a mention in a channel outside the channel allowlist", async () => {
    accessState.allowedChannelIds = ["C123"];
    new SlackInterface();

    await slackState.getEventHandler("app_mention")?.({
      event: {
        type: "app_mention",
        user: "U123",
        channel: "C404",
        ts: "1710000000.202",
        text: "<@UBOT123> run a task",
      },
      next: vi.fn().mockResolvedValue(undefined),
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("ignores a mention that has no prompt after the bot mention is removed", async () => {
    new SlackInterface();

    await slackState.getEventHandler("app_mention")?.({
      event: {
        type: "app_mention",
        user: "U123",
        channel: "C123",
        ts: "1710000000.203",
        text: "<@UBOT123>",
      },
      next: vi.fn().mockResolvedValue(undefined),
    });

    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("acknowledges and returns policy previews for /code policy orphans", async () => {
    const ack = vi.fn();
    frontalCodeApiState.getOrphanPolicy.mockResolvedValue({
      default_policy: { source: "default", approval_delay_secs: 60 },
      effective_policy: { source: "default", approval_delay_secs: 60 },
      configured_rules: [],
    });
    new SlackInterface();

    await slackState.getCommandHandler()?.({
      command: {
        text: "policy orphans",
        user_id: "U123",
        channel_id: "C123",
      },
      ack,
    });

    expect(frontalCodeApiState.getOrphanPolicy).toHaveBeenCalledWith({});
    expect(ack).toHaveBeenCalledWith(
      expect.objectContaining({
        response_type: "ephemeral",
      }),
    );
    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("acknowledges policy preview failures with an ephemeral error response", async () => {
    const ack = vi.fn();
    frontalCodeApiState.getOrphanPolicy.mockRejectedValue(new Error("policy unavailable"));
    new SlackInterface();

    await slackState.getCommandHandler()?.({
      command: {
        text: "policy orphans repo=frontal-code/slack",
        user_id: "U123",
        channel_id: "C123",
      },
      ack,
    });

    expect(ack).toHaveBeenCalledWith({
      response_type: "ephemeral",
      text: "Failed to load orphan policy: policy unavailable",
    });
    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("creates a task from /code commands after acknowledging", async () => {
    const ack = vi.fn();
    const registerSpy = vi
      .spyOn(
        SlackInterface.prototype as unknown as {
          registerTask: (...args: unknown[]) => Promise<void>;
        },
        "registerTask",
      )
      .mockResolvedValue(undefined);
    frontalCodeApiState.createTask.mockResolvedValue({
      task_id: "task-456",
      status: "running",
      message: "created",
    });
    new SlackInterface();

    await slackState.getCommandHandler()?.({
      command: {
        text: "Fix the flaky test",
        user_id: "U123",
        channel_id: "C123",
      },
      ack,
    });

    expect(ack).toHaveBeenCalledWith();
    expect(frontalCodeApiState.createTask).toHaveBeenCalledWith({
      prompt: "Fix the flaky test",
      user_id: "U123",
      channel_id: "C123",
      source: "slack",
    });
    expect(registerSpy).toHaveBeenCalledWith(
      expect.objectContaining({ task_id: "task-456" }),
      {
        taskId: "task-456",
        channelId: "C123",
        userId: "U123",
      },
      "Fix the flaky test",
    );
  });

  it("denies /code task creation from a user who is not allowlisted", async () => {
    const ack = vi.fn();
    new SlackInterface();

    await slackState.getCommandHandler()?.({
      command: {
        text: "Fix the flaky test",
        user_id: "U999",
        channel_id: "C123",
      },
      ack,
    });

    expect(ack).toHaveBeenCalledWith({
      response_type: "ephemeral",
      text: "You are not authorized to run Code tasks in this channel.",
    });
    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("denies /code from every user when the allowlist is empty, including policy previews", async () => {
    const ack = vi.fn();
    accessState.allowedUserIds = [];
    new SlackInterface();

    await slackState.getCommandHandler()?.({
      command: {
        text: "policy orphans",
        user_id: "U123",
        channel_id: "C123",
      },
      ack,
    });

    expect(ack).toHaveBeenCalledWith(expect.objectContaining({ response_type: "ephemeral" }));
    expect(frontalCodeApiState.getOrphanPolicy).not.toHaveBeenCalled();
    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("denies /code in a channel outside the channel allowlist", async () => {
    const ack = vi.fn();
    accessState.allowedChannelIds = ["C123"];
    new SlackInterface();

    await slackState.getCommandHandler()?.({
      command: {
        text: "Fix the flaky test",
        user_id: "U123",
        channel_id: "C404",
      },
      ack,
    });

    expect(ack).toHaveBeenCalledWith(expect.objectContaining({ response_type: "ephemeral" }));
    expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
  });

  it("acknowledges actions and forwards them to the Slack action handler", async () => {
    const ack = vi.fn();
    const handleSpy = vi
      .spyOn(
        SlackInterface.prototype as unknown as {
          handleSlackAction: (...args: unknown[]) => Promise<void>;
        },
        "handleSlackAction",
      )
      .mockResolvedValue(undefined);
    new SlackInterface();

    const body = {
      user: { id: "U123" },
      channel: { id: "C123" },
      message: { ts: "1710000000.100" },
    };
    const action = {
      action_id: "orphaned_hosted_agent.retry",
      value: "task-123",
    };

    await slackState.getActionHandler()?.({ action, ack, body });

    expect(ack).toHaveBeenCalledWith();
    expect(handleSpy).toHaveBeenCalledWith(action, body);
  });

  it("forwards Slack events to the connector event endpoint", async () => {
    new SlackInterface();

    await slackState.getEventHandler()?.({
      event: {
        type: "reaction_added",
        user: "U123",
        reaction: "eyes",
      },
      next: vi.fn().mockResolvedValue(undefined),
    });

    expect(frontalCodeApiState.sendConnectorEvent).toHaveBeenCalledWith("slack", {
      type: "reaction_added",
      userId: "U123",
      data: {
        type: "reaction_added",
        user: "U123",
        reaction: "eyes",
      },
    });
  });

  it("forwards Slack events without a user as connector events with an empty user id", async () => {
    new SlackInterface();

    await slackState.getEventHandler()?.({
      event: {
        type: "member_joined_channel",
        channel: "C123",
      },
      next: vi.fn().mockResolvedValue(undefined),
    });

    expect(frontalCodeApiState.sendConnectorEvent).toHaveBeenCalledWith("slack", {
      type: "member_joined_channel",
      userId: "",
      data: {
        type: "member_joined_channel",
        channel: "C123",
      },
    });
  });

  describe("OpenChannel Protocol host", () => {
    beforeEach(() => {
      // Earlier tests stub registerTask on the prototype; these run the real confirmation path.
      const prototype = SlackInterface.prototype as unknown as {
        registerTask: { mockRestore?: () => void };
      };
      prototype.registerTask.mockRestore?.();
    });

    function dmEnvelope(eventId: string) {
      return {
        type: "event_callback",
        event_id: eventId,
        event: {
          type: "message",
          text: "Investigate flaky test",
          user: "U123",
          channel: "D123",
          ts: "1710000000.100",
          channel_type: "im",
        },
      };
    }

    it("creates one task per Slack event even when Slack redelivers it", async () => {
      frontalCodeApiState.createTask.mockResolvedValue({
        task_id: "task-123",
        status: "running",
        message: "created",
      });
      slackState.chatPostMessage.mockResolvedValue({ ts: "1710000000.200" });
      new SlackInterface();
      const handler = slackState.getMessageHandler() as unknown as (args: {
        message: unknown;
        body: unknown;
      }) => Promise<void>;
      const envelope = dmEnvelope("Ev123");

      await handler({ message: envelope.event, body: envelope });
      await handler({ message: envelope.event, body: envelope });

      expect(frontalCodeApiState.createTask).toHaveBeenCalledTimes(1);
      // The confirmation is delivered by the ocp-sdk Slack sink into the DM thread.
      expect(slackState.chatPostMessage).toHaveBeenCalledTimes(1);
      expect(slackState.chatPostMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          channel: "D123",
          thread_ts: "1710000000.100",
          text: expect.stringContaining("Task created: task-123"),
        }),
      );
      expect(frontalCodeEventsState.trackTask).toHaveBeenCalledWith(
        expect.objectContaining({ taskId: "task-123", threadTs: "1710000000.100" }),
      );
    });

    it("replies in the thread when the control plane rejects a task", async () => {
      frontalCodeApiState.createTask.mockRejectedValue(new Error("quota exceeded"));
      slackState.chatPostMessage.mockResolvedValue({ ts: "1710000000.200" });
      new SlackInterface();
      const envelope = dmEnvelope("Ev124");

      await (
        slackState.getMessageHandler() as unknown as (args: {
          message: unknown;
          body: unknown;
        }) => Promise<void>
      )({ message: envelope.event, body: envelope });

      expect(slackState.chatPostMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          channel: "D123",
          thread_ts: "1710000000.100",
          text: "Code could not start this task: quota exceeded",
        }),
      );
      expect(frontalCodeEventsState.trackTask).not.toHaveBeenCalled();
    });

    it("answers an empty /code with usage instead of creating a task", async () => {
      const ack = vi.fn();
      new SlackInterface();

      await slackState.getCommandHandler()?.({
        command: { text: "   ", user_id: "U123", channel_id: "C123" },
        ack,
      });

      expect(ack).toHaveBeenCalledWith({
        response_type: "ephemeral",
        text: "Usage: /code <what you want done>",
      });
      expect(frontalCodeApiState.createTask).not.toHaveBeenCalled();
    });
  });
});
