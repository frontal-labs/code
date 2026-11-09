// Runs only when OCP_TEST_DATABASE_URL points at a disposable Postgres database, e.g.
//   OCP_TEST_DATABASE_URL=postgres://localhost:5432/ocp_test bun run test tests/ocp
import { randomUUID } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import { OcpBridge } from "../../src/bridge";
import type { Logger } from "../../src/log";
import { createOcpStore } from "../../src/store";

const databaseUrl = process.env.OCP_TEST_DATABASE_URL;

describe.skipIf(!databaseUrl)("OcpBridge with Postgres", () => {
  function createBridge(tableName: string) {
    const createTask = vi.fn().mockResolvedValue({ task_id: "task-pg", status: "pending" });
    const resolveTaskApproval = vi
      .fn()
      .mockResolvedValue({ task_id: "task-pg", status: "running" });
    const postMessage = vi.fn().mockResolvedValue({ ts: "1710000000.900" });
    const logger: Logger = { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() };
    const store = createOcpStore({ databaseUrl: databaseUrl as string, tableName });
    const bridge = new OcpBridge({
      api: { createTask, resolveTaskApproval },
      slack: { chat: { postMessage, update: vi.fn().mockResolvedValue({}) } },
      store,
      runMigrations: true,
      logger,
      onTaskCreated: async () => undefined,
    });
    return { bridge, store, createTask, resolveTaskApproval, logger };
  }

  it("migrates, records the audit trail, and dedupes Slack redelivery across restarts", async () => {
    const tableName = `ocp_events_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
    const envelope = {
      type: "event_callback",
      event_id: `Ev${randomUUID()}`,
      event: {
        type: "message",
        channel_type: "im",
        channel: "D123",
        user: "U123",
        text: "Investigate the flaky test",
        ts: "1710000000.100",
      },
    };

    const first = createBridge(tableName);
    await expect(first.bridge.receiveSlackEvent(envelope)).resolves.toBe(true);
    await first.bridge.resolveApproval({
      taskId: "task-pg",
      approvalKind: "orphaned_hosted_agent",
      action: "retry",
      slackUserId: "U123",
      resolvedBy: "U123",
      channelId: "D123",
    });
    await first.bridge.stop();

    // A new process on the same database must not run the redelivered event again.
    const second = createBridge(tableName);
    await expect(second.bridge.receiveSlackEvent(envelope)).resolves.toBe(false);
    expect(second.createTask).not.toHaveBeenCalled();

    const types = (await second.bridge.app.store.all()).map((event) => event.payload.case);
    expect(types.filter((type) => type === "messageReceived")).toHaveLength(2);
    expect(types.filter((type) => type === "actionExecuted")).toHaveLength(2);
    expect(first.createTask).toHaveBeenCalledTimes(1);
    expect(first.resolveTaskApproval).toHaveBeenCalledTimes(1);
    expect(first.logger.error).not.toHaveBeenCalled();

    await second.bridge.stop();
  });
});
