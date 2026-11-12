const vscode = require("vscode");
const { execFile } = require("node:child_process");
const { accessSync, constants } = require("node:fs");
const path = require("node:path");

// The two names every install channel provides for the same CLI.
const COMMAND_NAMES = ["frontal-code", "code"];

function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("frontal-code.startRepl", () => startRepl()),
    vscode.commands.registerCommand("frontal-code.askSelection", () => askSelection()),
    vscode.commands.registerCommand("frontal-code.askInput", () => askInput()),
  );
}

function deactivate() {
  // Intentionally empty: VS Code owns extension teardown.
}

function config() {
  return vscode.workspace.getConfiguration("frontal-code");
}

// Only the user or machine value is honored. A workspace .vscode/settings.json
// must not be able to point the extension at an arbitrary executable.
function frontalCodeCliPath() {
  const inspected = config().inspect("cliPath");
  const value = inspected?.globalValue ?? inspected?.defaultValue;
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : defaultCliCommand();
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

function frontalCodeModelArgs() {
  const model = config().get("defaultModel", "");
  if (typeof model === "string" && model.trim().length > 0) {
    return ["--model", model.trim()];
  }
  return [];
}

function startRepl() {
  if (!requireTrustedWorkspace()) {
    return;
  }
  // Launch the binary directly rather than typing the path into a shell.
  const terminal = vscode.window.createTerminal({
    name: "Frontal Code",
    shellPath: frontalCodeCliPath(),
    shellArgs: [],
  });
  terminal.show(true);
}

function requireTrustedWorkspace() {
  if (vscode.workspace.isTrusted) {
    return true;
  }
  vscode.window.showWarningMessage("Frontal Code: trust this workspace to run the CLI.");
  return false;
}

async function askInput() {
  const question = await vscode.window.showInputBox({
    prompt: "Ask Frontal Code",
    placeHolder: "Explain the active file",
  });
  if (!question?.trim()) {
    return;
  }
  await runFrontalCodePrompt(question.trim());
}

async function askSelection() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    vscode.window.showWarningMessage("Frontal Code: open an editor first.");
    return;
  }
  const selected = editor.document.getText(editor.selection).trim();
  if (!selected) {
    vscode.window.showWarningMessage("Frontal Code: select some text first.");
    return;
  }
  const question = await vscode.window.showInputBox({
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
  const output = vscode.window.createOutputChannel("Frontal Code");
  output.show(true);
  output.appendLine("Running Frontal Code...");

  const args = [...frontalCodeModelArgs(), "--output-format", "text", "prompt", prompt];
  const cwd = vscode.workspace.workspaceFolders?.[0]?.uri?.fsPath;
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
      vscode.window.showErrorMessage(message);
      return;
    }

    vscode.window.showInformationMessage("Frontal Code response ready.");
  });
}

module.exports = {
  activate,
  deactivate,
};
