// vscode-xazz/src/extension.ts — VS Code client for Xazz (issue #65, E1)
//
// Two responsibilities:
//   1. Start the `xazz-lsp` language server (diagnostics, hover, go-to-def,
//      rename) over stdio and wire it to `.xzz` documents.
//   2. Register run/check commands that invoke the `xazz` CLI on the current
//      file and surface the output in an OutputChannel.
//
// Binary discovery mirrors the Python binding: explicit config → `XAZZ_LSP_PATH`/
// `XAZZ_PATH` env → `target/{debug,release}` in any workspace folder → PATH.

import * as cp from "child_process";
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let output: vscode.OutputChannel;

/** Resolves a binary name, searching the configured path, env, target dirs, PATH. */
function resolveBinary(
  configured: string,
  envVar: string,
  name: string
): string | undefined {
  const exe = process.platform === "win32" ? `${name}.exe` : name;

  if (configured && fs.existsSync(configured)) {
    return configured;
  }
  const fromEnv = process.env[envVar];
  if (fromEnv && fs.existsSync(fromEnv)) {
    return fromEnv;
  }

  // Workspace target/{debug,release}
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const profile of ["debug", "release"]) {
      const cand = path.join(folder.uri.fsPath, "target", profile, exe);
      if (fs.existsSync(cand)) {
        return cand;
      }
    }
  }

  // PATH
  const dirs = (process.env.PATH ?? "").split(path.delimiter);
  for (const d of dirs) {
    const cand = path.join(d, exe);
    if (fs.existsSync(cand)) {
      return cand;
    }
  }
  return undefined;
}

function lspCommand(): { command: string; args: string[] } | undefined {
  const cfg = vscode.workspace.getConfiguration("xazz");
  const lsp = resolveBinary(cfg.get<string>("lspPath", ""), "XAZZ_LSP_PATH", "xazz-lsp");
  return lsp ? { command: lsp, args: [] } : undefined;
}

function cliCommand(): string | undefined {
  const cfg = vscode.workspace.getConfiguration("xazz");
  return resolveBinary(cfg.get<string>("cliPath", ""), "XAZZ_PATH", "xazz");
}

async function runCli(sub: "run" | "check"): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "xazz") {
    vscode.window.showWarningMessage("Open a .xzz file first.");
    return;
  }
  await editor.document.save();
  const cli = cliCommand();
  if (!cli) {
    vscode.window.showErrorMessage(
      "xazz CLI not found. Build it (cargo build -p xazz -p xazz-runner) or set xazz.cliPath."
    );
    return;
  }
  const file = editor.document.uri.fsPath;
  output.show(true);
  output.appendLine(`$ xazz ${sub} ${path.basename(file)}`);
  cp.execFile(cli, [sub, file], { maxBuffer: 64 * 1024 * 1024 }, (err, stdout, stderr) => {
    if (stdout) {
      output.append(stdout);
    }
    if (stderr) {
      output.append(stderr);
    }
    if (err) {
      output.appendLine(`[xazz] exited with ${err.code ?? "error"}`);
    }
  });
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output = vscode.window.createOutputChannel("Xazz");
  context.subscriptions.push(output);

  context.subscriptions.push(
    vscode.commands.registerCommand("xazz.run", () => runCli("run"))
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("xazz.check", () => runCli("check"))
  );

  const cmd = lspCommand();
  if (!cmd) {
    vscode.window.showWarningMessage(
      "xazz-lsp not found. Build it (cargo build -p xazz-lsp) or set xazz.lspPath."
    );
    return;
  }

  const serverOptions: ServerOptions = {
    run: { command: cmd.command, args: cmd.args, transport: TransportKind.stdio },
    debug: { command: cmd.command, args: cmd.args, transport: TransportKind.stdio },
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "xazz" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.xzz"),
    },
  };

  client = new LanguageClient("xazz", "Xazz Language Server", serverOptions, clientOptions);
  await client.start();
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}
