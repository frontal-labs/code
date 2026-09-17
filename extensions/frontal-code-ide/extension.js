const vscode = require("vscode");
const { execFile } = require("child_process");

function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("frontal-code.startRepl", () => startRepl()),
    vscode.commands.registerCommand("frontal-code.askSelection", () => askSelection()),
    vscode.commands.registerCommand("frontal-code.askInput", () => askInput())
  );
}

function deactivate() {}

function config() {
  return vscode.workspace.getConfiguration("frontal-code");
}

function frontal-codeCliPath() {
  const value = config().get("cliPath", "frontal-code");
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : "frontal-code";
}

function frontal-codeModelArgs() {
  const model = config().get("defaultModel", "");
  if (typeof model === "string" && model.trim().length > 0) {
    return ["--model", model.trim()];
  }
  return [];
}

function startRepl() {
  const terminal = vscode.window.createTerminal("Frontal Code");
  terminal.show(true);
  terminal.sendText(frontal-codeCliPath(), true);
}

async function askInput() {
  const question = await vscode.window.showInputBox({
    prompt: "Ask Frontal Code",
    placeHolder: "Explain the active file"
  });
  if (!question || !question.trim()) {
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
    placeHolder: "Explain this code"
  });
  if (!question || !question.trim()) {
    return;
  }
  const prompt = `${question.trim()}\n\nSelected code:\n${selected}`;
  await runFrontalCodePrompt(prompt);
}

async function runFrontalCodePrompt(prompt) {
  const output = vscode.window.createOutputChannel("Frontal Code");
  output.show(true);
  output.appendLine("Running Frontal Code...");

  const args = [...frontal-codeModelArgs(), "--output-format", "text", "prompt", prompt];
  const cwd = vscode.workspace.workspaceFolders?.[0]?.uri?.fsPath;
  const execOptions = cwd ? { cwd, maxBuffer: 16 * 1024 * 1024 } : { maxBuffer: 16 * 1024 * 1024 };

  const startedAt = Date.now();
  execFile(frontal-codeCliPath(), args, execOptions, (error, stdout, stderr) => {
    const elapsedMs = Date.now() - startedAt;
    output.appendLine(`Frontal Code finished in ${elapsedMs} ms.`);
    if (stdout && stdout.trim()) {
      output.appendLine("");
      output.appendLine(stdout.trimEnd());
    }
    if (stderr && stderr.trim()) {
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
  deactivate
};
