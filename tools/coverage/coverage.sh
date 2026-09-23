#!/usr/bin/env bash
# tools/coverage/coverage.sh — canonical coverage entrypoint.
# Delegated to by scripts/coverage.sh. Runs `bazel coverage //...`, then
# aggregates the lcov report with the `tools-coverage` Rust binary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="${OUT_DIR:-coverage}"
LCOV="$OUT_DIR/lcov.info"

echo "==> bazel coverage //..."
bazel coverage //... --combined_report=lcov --coverage_report_generator=@bazel_tools//tools/test/CoverageOutputGenerator/java/com/google/devtools/coverageoutputgenerator:Main "--@bazel_tools//tools/test:coverage_report_generator" 2>&1 | tail -5 || true

# Locate the produced lcov (Bazel writes it under bazel-testlogs by default).
if [ ! -f "$LCOV" ]; then
  GENERATED="$(bazel info bazel-testlogs 2>/dev/null)/coverage/lcov.info"
  mkdir -p "$OUT_DIR"
  if [ -f "$GENERATED" ]; then
    cp "$GENERATED" "$LCOV"
  fi
fi

if [ -f "$LCOV" ]; then
  echo "==> orbit-coverage summarize"
  cargo run -q -p tools-coverage -- summarize "$LCOV" "${@}"
else
  echo "no lcov report produced (did bazel coverage run?)"
fi
