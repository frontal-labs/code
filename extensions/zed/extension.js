const { zed } = require("@zed-industries/extension-api");
const { execFile } = require("node:child_process");
const { accessSync, constants } = require("node:fs");
const path = require("node:path");

// The two names every install channel provides for the same CLI.
const COMMAND_NAMES = ["frontal-code", "code"];

/** @param {import("@zed-industries/extension-api").ExtensionContext} context */
function activate(context) {
  context.subscriptions.push(
    zed.registerCommand("frontal-code.startRepl", () => startRepl()),
    zed.registerCommand("frontal-code.askSelection", () => askSelection()),
    zed.registerCommand("frontal-code.askInput", () => askInput()),
  );
}

function deactivate() {
  // Intentionally empty: Zed owns extension teardown.
}

function frontalCodeCliPath() {
  const inspected = zed.config().get("frontal-code.cliPath");
  const value =
    typeof inspected === "string" && inspected.trim().length > 0 ? inspected.trim() : defaultCliCommand();
  return value;
}

// `frontal-code` when it is on PATH, otherwise `code`: the same program under either name.
function defaultCliCommand() {
  for (const name of COMMAND_NAMES) {
    if (isOnPath(name)) {
      return name;
    }
  }
  return COMMAND_NAMES[0];
}

function isOnPath(name) {
  const dirs = (process.env.PATH || "").split(path.delimiter).filter((dir) => dir.length > 0);
  const suffixes = process.platform === "win32" ? [".exe", ""] : [""];
  return dirs.some((dir) =>
    suffixes.some((suffix) => {
      try {
        accessSync(path.join(dir, name + suffix), constants.X_OK);
        return true;
      } catch {
        return false;
      }
    }),
  );
}

function startRepl() {
  if (!requireTrustedWorkspace()) {
    return;
  }
  const terminal = zed.window.createTerminal({
    name: "Frontal Code",
    shellPath: frontalCodeCliPath(),
    shellArgs: [],
  });
  terminal.show(true);
}

function requireTrustedWorkspace() {
  if (zed.workspace.isTrusted) {
    return true;
  }
  zed.window.showWarningMessage("Frontal Code: trust this workspace to run the CLI.");
  return false;
}

async function askInput() {
  const question = await zed.window.showInputBox({
    prompt: "Ask Frontal Code",
    placeHolder: "Explain the active file",
  });
  if (!question?.trim()) {
    return;
  }
  await runFrontalCodePrompt(question.trim());
}

async function askSelection() {
  const editor = zed.window.activeTextEditor;
  if (!editor) {
    zed.window.showWarningMessage("Frontal Code: open an editor first.");
    return;
  }
  const selected = editor.document.getText(editor.selection).trim();
  if (!selected) {
    zed.window.showWarningMessage("Frontal Code: select some text first.");
    return;
  }
  const question = await zed.window.showInputBox({
    prompt: "What should Frontal Code do with this selection?",
    placeHolder: "Explain this code",
  });
  if (!question?.trim()) {
    return;
  }
  const prompt = `${question.trim()}\n\nSelected code:\n${selected}`;
  await runFrontalCodePrompt(prompt);
}

async function runFrontalCodePrompt(prompt) {
  if (!requireTrustedWorkspace()) {
    return;
  }
  const output = zed.window.createOutputChannel("Frontal Code");
  output.show(true);
  output.appendLine("Running Frontal Code...");

  const args = ["--output-format", "text", "prompt", prompt];
  const cwd = zed.workspace.workspaceFolders?.[0]?.uri?.fsPath;
  const execOptions = cwd ? { cwd, maxBuffer: 16 * 1024 * 1024 } : { maxBuffer: 16 * 1024 * 1024 };

  const startedAt = Date.now();
  execFile(frontalCodeCliPath(), args, execOptions, (error, stdout, stderr) => {
    const elapsedMs = Date.now() - startedAt;
    output.appendLine(`Frontal Code finished in ${elapsedMs} ms.`);
    if (stdout?.trim()) {
      output.appendLine("");
      output.appendLine(stdout.trimEnd());
    }
    if (stderr?.trim()) {
      output.appendLine("");
      output.appendLine("stderr:");
      output.appendLine(stderr.trimEnd());
    }

    if (error) {
      const message = `Frontal Code command failed: ${error.message}`;
      output.appendLine(message);
      zed.window.showErrorMessage(message);
      return;
    }

    zed.window.showInformationMessage("Frontal Code response ready.");
  });
}

module.exports = {
  activate,
  deactivate,
};
