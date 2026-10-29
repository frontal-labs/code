import { describe, expect, it, vi } from "vitest";
import { createOcpStore, type PgPoolLike } from "../../src/store";

function fakePool(): PgPoolLike & {
  query: ReturnType<typeof vi.fn>;
  end: ReturnType<typeof vi.fn>;
} {
  return {
    query: vi.fn().mockResolvedValue({ rows: [] }),
    end: vi.fn().mockResolvedValue(undefined),
  };
}

describe("createOcpStore", () => {
  it("runs the idempotent migration against the configured table", async () => {
    const pool = fakePool();
    const handle = createOcpStore(
      { databaseUrl: "postgres://localhost/ocp", tableName: "slack_events" },
      pool,
    );

    await handle.migrate();

    const statements = pool.query.mock.calls.map(([sql]) => String(sql)).join("\n");
    expect(statements).toMatch(/CREATE TABLE IF NOT EXISTS slack_events/i);
    expect(statements).toMatch(/ocp_actions/);
  });

  it("passes bound values as a mutable array to node-postgres", async () => {
    const pool = fakePool();
    const handle = createOcpStore(
      { databaseUrl: "postgres://localhost/ocp", tableName: "ocp_events" },
      pool,
    );

    await handle.store.has("event-1");

    const [, values] = pool.query.mock.calls[0];
    expect(values).toEqual(["event-1"]);
    expect(Object.isFrozen(values)).toBe(false);
  });

  it("reports reachability with select 1 and closes the pool", async () => {
    const pool = fakePool();
    const handle = createOcpStore(
      { databaseUrl: "postgres://localhost/ocp", tableName: "ocp_events" },
      pool,
    );

    await expect(handle.ping()).resolves.toBe(true);
    expect(pool.query).toHaveBeenCalledWith("select 1");

    pool.query.mockRejectedValueOnce(new Error("ECONNREFUSED"));
    await expect(handle.ping()).resolves.toBe(false);

    await handle.close();
    expect(pool.end).toHaveBeenCalledTimes(1);
  });
});
