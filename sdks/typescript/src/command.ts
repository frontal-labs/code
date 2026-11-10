import { accessSync, constants } from "node:fs";
import { delimiter, join } from "node:path";

/** The two names every install channel provides for the same CLI. */
const COMMAND_NAMES = ["frontal-code", "code"] as const;

/**
 * The CLI command to run when none is given: `frontal-code` if it is on `PATH`, otherwise
 * `code`. Both are the same program, so either works; the lookup only chooses which name to spawn.
 */
export function defaultCommand(env: NodeJS.ProcessEnv = process.env): string {
  for (const name of COMMAND_NAMES) {
    if (isOnPath(name, env)) {
      return name;
    }
  }
  return COMMAND_NAMES[0];
}

function isOnPath(name: string, env: NodeJS.ProcessEnv): boolean {
  const dirs = (env.PATH ?? "").split(delimiter).filter((dir) => dir.length > 0);
  const suffixes = process.platform === "win32" ? [".exe", ""] : [""];
  return dirs.some((dir) =>
    suffixes.some((suffix) => {
      try {
        accessSync(join(dir, `${name}${suffix}`), constants.X_OK);
        return true;
      } catch {
        return false;
      }
    }),
  );
}
