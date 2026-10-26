# Frontal Code Types

Foundational shared types for the Frontal Code workspace.

## Overview

This crate owns the shared types used across `code-telemetry`, `code-runtime`,
`code-memory`, and `code-providers`. It stays dependency-minimal (only `serde`)
so it cannot introduce dependency cycles in the workspace.

## Key Components

- **ID newtypes**: `SessionId`, `WorkspaceId`, `RepoId`, `LaneId`, `TaskId`,
  `WorkItemId`, `CheckoutId` — transparent `String` wrappers.
- **ClientIdentity**: client identity used for telemetry and AI provider requests.
- **Usage**: `TokenUsage`, `UsageCostEstimate`, `ModelPricing` for cost estimation.
- **MemoryScope**: session/repo/branch scoping for memory operations.

## Dependencies

- `serde` for serialization
