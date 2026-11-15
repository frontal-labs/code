---
'@frontal-labs/frontal-code': patch
---

Fold the hosted server's local persistence into `.frontal-code/server/`, matching how every other runtime artifact already lives under `.frontal-code`. The default state file moves from `.code-server/state.json` to `.frontal-code/server/state.json`, and the default workspace root from `.code-server/workspaces` to `.frontal-code/server/workspaces`. Existing `.code-server` data is not migrated; deployments with prior state start with a fresh state file.
