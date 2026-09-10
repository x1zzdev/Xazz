# Xazz for VS Code (issue #65, E1)

Language support for `.xzz` Xazz pipeline scripts:

- **Diagnostics** — the same line:col errors as `xazz check` (the server reuses
  the Rust checker, so editor and CLI agree byte-for-byte).
- **Hover / Go-to-definition / Rename** — powered by the `xazz-lsp` symbol table.
- **Run / Check commands** — invoke the `xazz` CLI on the active file and stream
  output to the *Xazz* output channel.
- **Syntax highlighting** for the `.xzz` grammar.

## Requirements

Build the two binaries first (from the repo root):

```bash
cargo build -p xazz-lsp -p xazz -p xazz-runner
```

The extension auto-discovers them in `target/{debug,release}` of any workspace
folder, then on `PATH`. You can also set:

- `xazz.lspPath` — absolute path to `xazz-lsp`
- `xazz.cliPath` — absolute path to `xazz`

(or the `XAZZ_LSP_PATH` / `XAZZ_PATH` environment variables).

## Build & install (development)

```bash
cd vscode-xazz
npm install
npm run compile
# then press F5 in VS Code to launch an Extension Development Host
```

To produce a `.vsix`:

```bash
npm install -g @vscode/vsce
vsce package
code --install-extension xazz-0.1.0.vsix
```

## Commands

| Command | Description |
|---|---|
| `Xazz: Run Pipeline` | `xazz run <active file>` |
| `Xazz: Check` | `xazz check <active file>` |

The LSP client (`vscode-languageclient`) starts `xazz-lsp` over stdio for every
`.xzz` document in the workspace.
