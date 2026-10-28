// Durable OCP event store backed by Postgres (`ocp-sdk/postgres`). The pool is created eagerly but
// connects lazily, so constructing the store never blocks; `migrate()` runs the idempotent schema.
import type { OpenChannelOptions } from "ocp-sdk";
import { migratePostgresStore, type PostgresClient, postgresStore } from "ocp-sdk/postgres";
import pg from "pg";

// The SDK root does not export `EventStore`; derive it from the option that consumes it.
export type EventStore = OpenChannelOptions["store"];

export interface OcpStoreOptions {
  databaseUrl: string;
  tableName: string;
}

export interface OcpStoreHandle {
  readonly store: EventStore;
  migrate(): Promise<void>;
  ping(): Promise<boolean>;
  close(): Promise<void>;
}

export interface PgPoolLike {
  query(sql: string, values?: unknown[]): Promise<{ rows: unknown[] }>;
  end(): Promise<void>;
}

export function createPgPool(databaseUrl: string): PgPoolLike {
  return new pg.Pool({ connectionString: databaseUrl, max: 5 }) as unknown as PgPoolLike;
}

// node-postgres types `values` as a mutable array; the SDK hands over a readonly one.
function toPostgresClient(pool: PgPoolLike): PostgresClient {
  return {
    query: async <TRow>(sql: string, values?: readonly unknown[]) => {
      const result = await pool.query(sql, values ? [...values] : undefined);
      return { rows: result.rows as TRow[] };
    },
  };
}

export function createOcpStore(
  options: OcpStoreOptions,
  pool: PgPoolLike = createPgPool(options.databaseUrl),
): OcpStoreHandle {
  const client = toPostgresClient(pool);
  return {
    store: postgresStore({ client, tableName: options.tableName }),
    migrate: () => migratePostgresStore(client, { tableName: options.tableName }),
    ping: async () => {
      try {
        await pool.query("select 1");
        return true;
      } catch {
        return false;
      }
    },
    close: () => pool.end(),
  };
}
