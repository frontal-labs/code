# Frontal Code Zed Extension

Local Frontal Code integration for the [Zed](https://zed.dev) editor.

## Commands

- `frontal-code.startRepl` — Start the Frontal Code REPL
- `frontal-code.askInput` — Ask Frontal Code a question
- `frontal-code.askSelection` — Ask Frontal Code about the current selection

## Settings

- `frontal-code.cliPath` (default: `frontal-code` from PATH, or `code`)

## Installation

Zed discovers extensions placed in `~/.config/zed/extensions/` or workspace-level `extensions/` directories. Copy or symlink the contents of this package into your Zed extensions directory, or use `zed extension install` from a published marketplace entry.

## `/ide` Integration

`/ide` currently supports `vscode`, `cursor`, `antigravity`, and `windsurf`. Zed support will be added as a future target.
