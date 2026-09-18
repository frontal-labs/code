# Frontal Code

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Detected stack
- Languages: Rust.
- Frameworks: none detected from the supported starter markers.

## Verification
- Run verification from the repo root through the Bazel wrappers: `bazel run //:fmt`, `bazel test //:fmt_check`, `bazel test //:lint`, `bazel test //:test`. The Rust wrappers invoke Cargo; the aggregate suites also cover SDK and launcher targets.

## Repository shape
- `crates/` contains the Rust workspace crates and active CLI/runtime implementation.
- `tools/` contains the repo-level developer tooling suite (doctor, coverage, benchmark, cache, codegen, fuzz, generators, remote, telemetry, templates, version, workspace). Each tool is a Cargo workspace member; `scripts/` are thin delegators and Bazel exposes runnable tools as `//tools/<tool>:<tool>` targets (`templates` is a library only). See `docs/TOOLS.md` for the full reference.
- `docs/` and top-level Markdown files track behavior, parity, and operational guidance.

## Dev tooling
- Run a tool with `bazel run //tools/<tool>:<tool>` (e.g. `bazel run //tools/doctor:doctor`) which delegates to `cargo run -p tools-<tool>` internally.
- Adding a new Rust tool: create `tools/<name>/` with a `Cargo.toml` (`name = "tools-<name>"`), `src/main.rs`, and a `BUILD.bazel` `sh_binary` wrapping `//tools:cargo_run.sh` (see `tools/BUILD.bazel`). Register it in root `Cargo.toml` members and in `.frontal-code/settings.json` `devtools`.

## Working agreement
- Prefer small, reviewable changes and keep generated bootstrap files aligned with actual repo workflows.
- Keep shared defaults in `.frontal-code/settings.json`; reserve `.frontal-code/settings.local.json` for machine-local overrides.
- Do not overwrite existing `FCODE.md` content automatically; update it intentionally when repo workflows change.
