import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    preserveSymlinks: true,
  },
  test: {
    environment: "node",
    include: ["tests/**/*.test.ts"],
    // Tests spawn the mock CLI as a real process. Under Bazel's parallel load a spawn can take
    // several seconds, so the default 5s timeout produced flaky failures.
    testTimeout: 30_000,
  },
  server: {
    // Bazel exposes source files through runfiles symlinks; Vitest 3 otherwise
    // rejects the resolved source paths before loading the test modules.
    fs: {
      strict: false,
    },
  },
});
