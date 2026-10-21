import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["tests/**/*.test.ts"],
  },
  server: {
    // Bazel exposes source files through runfiles symlinks; Vitest 3 otherwise
    // rejects the resolved source paths before loading the test modules.
    fs: {
      strict: false,
    },
  },
});
