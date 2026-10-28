// Process entrypoint. Kept separate from src/index.ts so importing the app never starts it; Node 22
// has no `import.meta.main` and the package is ESM, so `require.main` is unavailable too.
import main from "./index";

main().catch((error) => {
  console.error("Failed to start application:", error);
  process.exit(1);
});
