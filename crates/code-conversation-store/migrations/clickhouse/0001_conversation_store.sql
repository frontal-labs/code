-- Phase 3 schema for the conversation store (docs/design/conversation-store.md).
-- Every statement is idempotent (IF NOT EXISTS). Statements are separated by
-- semicolons, and `--` starts a comment everywhere. Do not put semicolons or
-- `--` inside string literals.
--
-- Read-path contract (not enforced by the schema):
-- * messages and tool_calls keep duplicate keys until a merge. Every query must
--   read them with FINAL (or argMax), or duplicates will show.
-- * Dedupe happens only inside one partition. An event must carry the same
--   created_at on every delivery of its key, so that it lands in the same partition.

CREATE TABLE IF NOT EXISTS sessions
(
    user_id         String,
    session_id      String,
    repo_id         String,
    branch          LowCardinality(String),
    client          LowCardinality(String),
    client_version  String,
    model           LowCardinality(String),
    provider        LowCardinality(String),
    forked_from     Nullable(String),
    created_at      Nullable(DateTime64(3, 'UTC')),
    version         UInt64,
    ingested_at     DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (user_id, session_id);

CREATE TABLE IF NOT EXISTS messages
(
    user_id         String,
    session_id      String,
    seq             UInt64,
    block_idx       UInt16,
    event_id        UUID,
    role            LowCardinality(String),
    kind            LowCardinality(String),
    text            String,
    model           LowCardinality(String),
    provider        LowCardinality(String),
    input_tokens    UInt64,
    output_tokens   UInt64,
    legacy          Bool,
    created_at      Nullable(DateTime64(3, 'UTC')),
    ingested_at     DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(ingested_at)
-- Legacy rows have NULL created_at. ClickHouse rejects a Nullable partition key,
-- so they fall into the epoch partition (197001) instead.
PARTITION BY toYYYYMM(ifNull(created_at, toDateTime64(0, 3, 'UTC')))
ORDER BY (user_id, session_id, seq, block_idx);

CREATE TABLE IF NOT EXISTS tool_calls
(
    user_id         String,
    session_id      String,
    seq             UInt64,
    block_idx       UInt16,
    tool_name       LowCardinality(String),
    is_error        Bool,
    output_head     String,
    output_tail     String,
    original_bytes  UInt64,
    created_at      Nullable(DateTime64(3, 'UTC')),
    ingested_at     DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(ingested_at)
-- Legacy rows have NULL created_at. ClickHouse rejects a Nullable partition key,
-- so they fall into the epoch partition (197001) instead.
PARTITION BY toYYYYMM(ifNull(created_at, toDateTime64(0, 3, 'UTC')))
ORDER BY (user_id, session_id, seq, block_idx);

CREATE TABLE IF NOT EXISTS compactions
(
    user_id              String,
    session_id           String,
    seq                  UInt64,
    summary              String,
    removed_count        UInt32,
    archived_messages    String,
    created_at           Nullable(DateTime64(3, 'UTC')),
    ingested_at          DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(ingested_at)
ORDER BY (user_id, session_id, seq);

CREATE TABLE IF NOT EXISTS embedding_models
(
    model           String,
    provider        LowCardinality(String),
    dimension       UInt32,
    revision        String,
    active          Bool,
    updated_at      DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (model, revision);
