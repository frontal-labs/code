// Standalone OCP schema migration, for deploy steps that run before the bot starts (Kubernetes init
// containers, Fly release commands). It reads only the OCP variables so it needs no Slack secrets.
import { z } from "zod";
import { createOcpStore } from "./store";

const migrationEnv = z.object({
  OCP_DATABASE_URL: z.string().regex(/^postgres(ql)?:\/\//, "must be a postgres:// URL"),
  OCP_EVENTS_TABLE: z
    .string()
    .regex(/^[a-z_][a-z0-9_]*$/)
    .default("ocp_events"),
});

async function migrate(): Promise<void> {
  const env = migrationEnv.parse({
    OCP_DATABASE_URL: process.env.OCP_DATABASE_URL,
    OCP_EVENTS_TABLE: process.env.OCP_EVENTS_TABLE || undefined,
  });
  const store = createOcpStore({
    databaseUrl: env.OCP_DATABASE_URL,
    tableName: env.OCP_EVENTS_TABLE,
  });
  try {
    await store.migrate();
    console.log(`OCP event store migrated (table ${env.OCP_EVENTS_TABLE})`);
  } finally {
    await store.close();
  }
}

migrate().catch((error) => {
  console.error("OCP migration failed:", error);
  process.exit(1);
});
