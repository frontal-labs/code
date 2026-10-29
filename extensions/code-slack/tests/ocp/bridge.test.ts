import { describe, expect, it, vi } from "vitest";
import { OcpBridge } from "../../src/bridge";
import type { Logger } from "../../src/log";
import { blocksFromAttachments } from "../../src/transport";
import type { FrontalCodeTask, SlackBlock } from "../../src/types";
import { MemoryEventStore, memoryStoreHandle } from "../helpers/memory-event-store";

function createHarness() {
  const store = new MemoryEventStore();
  const storeHandle = memoryStoreHandle(store);
  const closeStore = vi.spyOn(storeHandle, "close");
  const migrate = vi.spyOn(storeHandle, "migrate");
  let ts = 0;
  const postMessage = vi.fn(async () => ({ ts: `1710000000.${String(++ts).padStart(3, "0")}` }));
  const update = vi.fn().mockResolvedValue({ ok: true });
  const createTask = vi.fn().mockResolvedValue({
    task_id: "task-1",
    status: "pending",
    message: "created",
  });
  const resolveTaskApproval = vi.fn().mockResolvedValue({
    task_id: "task-1",
    status: "running",
    worker_status: "running",
  } as FrontalCodeTask);
  const onTaskCreated = vi.fn().mockResolvedValue(undefined);
  const logger: Logger = { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() };

  const bridge = new OcpBridge({
    api: { createTask, resolveTaskApproval },
    slack: { chat: { postMessage, update } },
    store: storeHandle,
    runMigrations: true,
    logger,
    onTaskCreated,
  });

  return {
    bridge,
    store,
    closeStore,
    migrate,
    postMessage,
    update,
    createTask,
    resolveTaskApproval,
    onTaskCreated,
    logger,
  };
}

function directMessageEnvelope(overrides: Record<string, unknown> = {}) {
  return {
    type: "event_callback",
    event_id: "Ev001",
    event: {
      type: "message",
      channel_type: "im",
      channel: "D123",
      user: "U123",
      text: "Investigate the flaky test",
      ts: "1710000000.100",
      ...overrides,
    },
  };
}

describe("OcpBridge", () => {
  it("turns a Slack DM into a policy-checked task creation and records it", async () => {
    const h = createHarness();

    await expect(h.bridge.receiveSlackEvent(directMessageEnvelope())).resolves.toBe(true);

    expect(h.migrate).toHaveBeenCalledTimes(1);
    expect(h.createTask).toHaveBeenCalledWith({
      prompt: "Investigate the flaky test",
      user_id: "U123",
      channel_id: "D123",
      thread_ts: "1710000000.100",
      source: "slack",
    });
    expect(h.onTaskCreated).toHaveBeenCalledWith(
      expect.objectContaining({ task_id: "task-1" }),
      { taskId: "task-1", channelId: "D123", threadTs: "1710000000.100", userId: "U123" },
      "Investigate the flaky test",
    );
    expect(h.store.eventTypes()).toEqual(
      expect.arrayContaining(["message.received", "policy.allowed", "action.executed"]),
    );
    const inbound = h.store.events.find((event) => event.eventId === "Ev001");
    expect(inbound?.actor?.id).toBe("slack:U123");
    expect(inbound?.recipient?.id).toBe("agent:frontal-code");
  });

  it("ignores Slack retries of the same event", async () => {
    const h = createHarness();

    await h.bridge.receiveSlackEvent(directMessageEnvelope());
    await expect(h.bridge.receiveSlackEvent(directMessageEnvelope())).resolves.toBe(false);

    expect(h.createTask).toHaveBeenCalledTimes(1);
  });

  it("drops bot messages in the Slack normalizer", async () => {
    const h = createHarness();

    await expect(
      h.bridge.receiveSlackEvent(directMessageEnvelope({ bot_id: "B123" })),
    ).resolves.toBe(false);

    expect(h.createTask).not.toHaveBeenCalled();
    expect(h.store.events).toHaveLength(0);
  });

  it("starts top-level tasks from prompts without a thread and strips bot mentions", async () => {
    const h = createHarness();

    await h.bridge.receiveSlackPrompt({
      eventId: "slack-command:trigger-1",
      userId: "U123",
      channelId: "C123",
      text: "<@U0BOT> ship the fix",
    });

    expect(h.createTask).toHaveBeenCalledWith({
      prompt: "ship the fix",
      user_id: "U123",
      channel_id: "C123",
      source: "slack",
    });
    expect(h.onTaskCreated).toHaveBeenCalledWith(
      expect.anything(),
      { taskId: "task-1", channelId: "C123", threadTs: undefined, userId: "U123" },
      "ship the fix",
    );
  });

  it("does not create a task for an empty prompt", async () => {
    const h = createHarness();

    await h.bridge.receiveSlackPrompt({ userId: "U123", channelId: "C123", text: "<@U0BOT>" });

    expect(h.createTask).not.toHaveBeenCalled();
  });

  it("replies in the thread and records action.failed when the control plane rejects a task", async () => {
    const h = createHarness();
    h.createTask.mockRejectedValueOnce(new Error("control plane unavailable"));

    await h.bridge.receiveSlackEvent(directMessageEnvelope());

    expect(h.onTaskCreated).not.toHaveBeenCalled();
    expect(h.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        channel: "D123",
        thread_ts: "1710000000.100",
        text: expect.stringContaining("control plane unavailable"),
      }),
    );
    expect(h.store.eventTypes()).toEqual(expect.arrayContaining(["action.failed", "message.sent"]));
  });

  it("resolves approvals through the policy-controlled action and records the decision", async () => {
    const h = createHarness();

    const task = await h.bridge.resolveApproval({
      eventId: "slack-action:U123:1710000001.000",
      taskId: "task-1",
      approvalKind: "orphaned_hosted_agent",
      action: "retry",
      slackUserId: "U123",
      resolvedBy: "U123",
      reason: "resolved from Slack approval action",
      channelId: "C123",
      threadTs: "1710000000.100",
    });

    expect(task.task_id).toBe("task-1");
    expect(h.resolveTaskApproval).toHaveBeenCalledWith({
      taskId: "task-1",
      approvalKind: "orphaned_hosted_agent",
      action: "retry",
      resolvedBy: "U123",
      reason: "resolved from Slack approval action",
    });
    const decision = h.store.events.find(
      (event) => event.eventId === "slack-action:U123:1710000001.000",
    );
    expect(decision?.actor?.id).toBe("slack:U123");
    const executed = h.store.events.find((event) => event.payload.case === "actionExecuted");
    expect(executed?.correlationId).toBe("frontal-code-task:task-1");
    expect(h.createTask).not.toHaveBeenCalled();
  });

  it("rethrows approval failures and refuses to replay the same click", async () => {
    const h = createHarness();
    h.resolveTaskApproval.mockRejectedValueOnce(new Error("approval already resolved upstream"));
    const decision = {
      eventId: "slack-action:U123:1710000002.000",
      taskId: "task-1",
      approvalKind: "github_review_followup" as const,
      action: "ack" as const,
      slackUserId: "U123",
      resolvedBy: "alice",
      channelId: "C123",
    };

    await expect(h.bridge.resolveApproval(decision)).rejects.toThrow(
      "approval already resolved upstream",
    );
    await expect(h.bridge.resolveApproval(decision)).rejects.toThrow(/already processed/);
    expect(h.resolveTaskApproval).toHaveBeenCalledTimes(1);
  });

  it("fails approval decisions whose input does not validate", async () => {
    const h = createHarness();

    await expect(
      h.bridge.resolveApproval({
        taskId: "task-1",
        approvalKind: "not-a-kind" as never,
        action: "retry",
        slackUserId: "U123",
        resolvedBy: "U123",
        channelId: "C123",
      }),
    ).rejects.toThrow();
    expect(h.resolveTaskApproval).not.toHaveBeenCalled();
  });

  it("delivers agent messages with Block Kit through the Slack sink and returns the ts", async () => {
    const h = createHarness();
    const blocks: SlackBlock[] = [{ type: "section", text: { type: "mrkdwn", text: "*done*" } }];

    const response = await h.bridge.sendToThread({
      channelId: "C123",
      threadTs: "1710000000.100",
      text: "Task completed",
      blocks,
      correlationId: "frontal-code-task:task-1",
    });

    expect(response).toEqual({ ts: "1710000000.001" });
    expect(h.postMessage).toHaveBeenCalledWith({
      channel: "C123",
      text: "Task completed",
      thread_ts: "1710000000.100",
      blocks,
    });
    const sent = h.store.events.find((event) => event.payload.case === "messageSent");
    expect(sent?.correlationId).toBe("frontal-code-task:task-1");
    expect(sent?.actor?.id).toBe("agent:frontal-code");
  });

  it("updates Slack messages directly and records the edit for audit", async () => {
    const h = createHarness();
    const blocks: SlackBlock[] = [{ type: "section", text: { type: "mrkdwn", text: "resolved" } }];

    await h.bridge.updateMessage({ channelId: "C123", ts: "1710000000.500", text: "ok", blocks });

    expect(h.update).toHaveBeenCalledWith({
      channel: "C123",
      ts: "1710000000.500",
      text: "ok",
      blocks,
    });
    const audit = h.store.events.find((event) => event.payload.case === "messageSent");
    expect(audit?.payload.case).toBe("messageSent");
    if (audit?.payload.case === "messageSent") {
      expect(audit.payload.value.metadata).toEqual({ kind: "update", slack_ts: "1710000000.500" });
      expect(blocksFromAttachments(audit.payload.value.attachments)).toEqual(blocks);
    }
    expect(h.postMessage).not.toHaveBeenCalled();
  });

  it("keeps a Slack edit even when the audit write fails", async () => {
    const h = createHarness();
    vi.spyOn(h.store, "appendIfAbsent").mockRejectedValue(new Error("database down"));

    await expect(
      h.bridge.updateMessage({ channelId: "C123", ts: "1710000000.500", text: "ok" }),
    ).resolves.toBeUndefined();

    expect(h.update).toHaveBeenCalled();
    expect(h.logger.warn).toHaveBeenCalledWith(
      "Failed to record OCP audit event",
      expect.objectContaining({ error: "database down" }),
    );
  });

  it("starts once, stops the app, and closes the store", async () => {
    const h = createHarness();

    await Promise.all([h.bridge.start(), h.bridge.start()]);
    await h.bridge.stop();

    expect(h.migrate).toHaveBeenCalledTimes(1);
    expect(h.closeStore).toHaveBeenCalledTimes(1);
    await expect(h.bridge.ping()).resolves.toBe(true);
  });
});
