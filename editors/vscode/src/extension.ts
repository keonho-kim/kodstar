// Purpose: VS Code 활성·해제 경계에서 명령과 소유 CLI 세션 연결.
// Pattern: Composition Root.
// Usage: VS Code workspace extension host가 activate·deactivate 호출.
// Related: editors/vscode/src/commands.ts, editors/vscode/package.json.

import * as vscode from "vscode";
import { Commands } from "./commands";

// VS Code calls deactivate without context; this handle owns only the current adapter lifecycle.
let commands: Commands | undefined;
export function activate(context: vscode.ExtensionContext): void {
    commands = new Commands(context);
}
export async function deactivate(): Promise<void> {
    const current = commands;
    commands = undefined;
    await current?.close();
}
