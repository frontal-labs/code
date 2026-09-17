# Frontal Code IDE Extension

Local Frontal Code integration for VS Code and Cursor.

## Commands

- `Frontal Code: Start REPL`
- `Frontal Code: Ask Frontal Code`
- `Frontal Code: Ask About Selection`

## Settings

- `frontal-code.cliPath` (default: `frontal-code`)
- `frontal-code.defaultModel` (default: empty)

## `/ide` Integration

`/ide vscode`, `/ide cursor`, `/ide antigravity`, and `/ide windsurf` now:

- package this extension into `.frontal-code/extensions/frontal-code-ide-<version>.vsix`
- install it into the target editor via editor CLI (`--install-extension ... --force`)
- write editor integration config (`.vscode/frontal-code.json` or `.cursor/frontal-code.json`)
- launch the editor at the current workspace root

If install fails, `/ide` reports the exact install error while still attempting editor launch.
