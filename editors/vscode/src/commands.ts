// Purpose: 파일·객체 선택·필터·진행·취소와 실제 위치 이동 연결.
// Pattern: UI Coordinator.
// Usage: extension 진입점이 명령을 등록하고 해제 시 close를 기다림.
// Related: editors/vscode/src/client.ts, editors/vscode/src/view.ts, editors/vscode/src/positions.ts.

import * as vscode from "vscode";
import { realpath } from "node:fs/promises";
import * as path from "node:path";
import { SessionClient, SessionFailure } from "./client";
import { isSymbol, Result, SymbolRow } from "./protocol";
import { editFilters, Filters } from "./filters";
import { utf16Column } from "./positions";
import { Location, RelationsView } from "./view";

export class Commands {
    readonly view = new RelationsView();
    private client?: SessionClient;
    private closing?: Promise<void>;
    private filters: Filters = {
        relations: [],
        include_candidates: false,
        include_external: false,
    };

    constructor(context: vscode.ExtensionContext) {
        context.subscriptions.push(this.view);
        for (const command of [
            "build",
            "symbols",
            "related",
            "deps",
            "refs",
            "explain",
            "impact",
        ] as const) {
            context.subscriptions.push(
                vscode.commands.registerCommand(`kodstar.${command}`, () =>
                    this.handle(command).catch((error) => this.error(error)),
                ),
            );
        }
        context.subscriptions.push(
            vscode.commands.registerCommand("kodstar.filters", () =>
                this.configure().catch((error) => this.error(error)),
            ),
            vscode.commands.registerCommand("kodstar.restart", () => this.close()),
            vscode.commands.registerCommand("kodstar.openLocation", (location: Location) =>
                this.open(location).catch((error) => this.error(error)),
            ),
        );
    }

    private async session(root: string): Promise<SessionClient> {
        await this.closing;
        const current = this.client;
        if (current?.closed) {
            await current.close();
            if (this.client === current) this.client = undefined;
        }
        await this.closing;
        if (!this.client) {
            const executable = vscode.workspace
                .getConfiguration("kodstar", vscode.Uri.file(root))
                .get<string>("executable", "kodstar");
            this.client = new SessionClient(executable, root);
        }
        return this.client;
    }

    private async folder(): Promise<vscode.WorkspaceFolder> {
        const editor = vscode.window.activeTextEditor;
        const current = editor && vscode.workspace.getWorkspaceFolder(editor.document.uri);
        const folder =
            current ??
            (vscode.workspace.workspaceFolders?.length === 1
                ? vscode.workspace.workspaceFolders[0]
                : await vscode.window.showWorkspaceFolderPick());
        if (!folder || folder.uri.scheme !== "file") throw new vscode.CancellationError();
        return folder;
    }

    private async request(
        root: string,
        method: string,
        params: unknown,
        title: string,
    ): Promise<Result> {
        const client = await this.session(root);
        this.view.reset(title);
        try {
            const result = await vscode.window.withProgress(
                { location: vscode.ProgressLocation.Notification, title, cancellable: true },
                async (progress, token) => {
                    const task = client.start(method, params, (event) =>
                        progress.report({
                            message: `${event.phase ?? "working"}: ${event.completed_files ?? 0} files`,
                        }),
                    );
                    const cancellation = token.onCancellationRequested(task.cancel);
                    try {
                        return await task.promise;
                    } finally {
                        cancellation.dispose();
                    }
                },
            );
            await this.view.show(result, root);
            return result;
        } catch (error) {
            if (error instanceof SessionFailure) await this.view.show(error.result, root);
            throw error;
        }
    }

    private async handle(
        command: "build" | "symbols" | "related" | "deps" | "refs" | "explain" | "impact",
    ): Promise<void> {
        const folder = await this.folder();
        const root = folder.uri.fsPath;
        if (command === "build") {
            await this.request(root, "build", { project: { root } }, "KodStar: build index");
            return;
        }
        const document = vscode.window.activeTextEditor?.document;
        if (
            !document ||
            document.uri.scheme !== "file" ||
            vscode.workspace.getWorkspaceFolder(document.uri) !== folder
        )
            throw new Error("Open a saved source file in this workspace");
        if (document.isDirty)
            throw new Error("KodStar analyzes saved files. Save this file before querying");
        const file = document.uri.fsPath;
        const symbols = await this.request(
            root,
            "query",
            { project: { root }, query: { kind: "symbols", file }, options: { limit: 1000 } },
            "KodStar: symbols",
        );
        if (command === "symbols") return;
        const candidates = symbols.rows.filter(isSymbol);
        const picked = await vscode.window.showQuickPick(
            candidates.map((symbol) => ({
                label: symbol.qualified_name,
                description: `${symbol.kind} · line ${symbol.definition.line}`,
                symbol,
            })),
            { placeHolder: "Choose the exact object definition" },
        );
        if (!picked) return;
        const symbol: SymbolRow = picked.symbol;
        const options = {
            ...this.filters,
            direction:
                command === "deps"
                    ? "out"
                    : command === "refs" || command === "impact"
                      ? "in"
                      : this.filters.direction,
        };
        if (command === "explain") {
            delete options.depth;
            delete options.transitive;
        }
        await this.request(
            root,
            "query",
            {
                project: { root },
                query: {
                    kind: command,
                    file,
                    name: symbol.qualified_name,
                    at: [symbol.definition.line, symbol.definition.column],
                },
                options,
            },
            `KodStar: ${command}`,
        );
    }

    private async configure(): Promise<void> {
        const next = await editFilters(this.filters);
        if (next) {
            this.filters = next;
            this.view.message("Filters updated. Run a relationship query to apply them");
        }
    }

    private async open(location: Location): Promise<void> {
        if (location.encoded) throw new Error("Use the CLI for this non-UTF-8 path");
        const root = await realpath(location.root);
        const target = await realpath(path.resolve(root, location.file));
        const relative = path.relative(root, target);
        if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative))
            throw new Error("Source location is outside the indexed root");
        const document = await vscode.workspace.openTextDocument(vscode.Uri.file(target));
        if (document.isDirty)
            throw new Error(
                "This location refers to the saved file. Save and rerun the query before navigating",
            );
        const line = location.span.line - 1;
        if (!Number.isSafeInteger(line) || line < 0 || line >= document.lineCount)
            throw new Error("Source changed; rerun the query");
        const column = utf16Column(document.lineAt(line).text, location.span.column);
        const editor = await vscode.window.showTextDocument(document);
        const position = new vscode.Position(line, column);
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(
            new vscode.Range(position, position),
            vscode.TextEditorRevealType.InCenterIfOutsideViewport,
        );
    }

    private error(error: unknown): void {
        if (error instanceof vscode.CancellationError) return;
        if (error instanceof SessionFailure && error.code === 130) {
            this.view.message("Cancelled; results may be incomplete");
            return;
        }
        const message = error instanceof Error ? error.message : String(error);
        this.view.message(message);
        void vscode.window.showErrorMessage(message);
    }
    async close(): Promise<void> {
        if (this.closing) return this.closing;
        const client = this.client;
        this.client = undefined;
        if (client) {
            this.closing = client.close().finally(() => {
                this.closing = undefined;
            });
            await this.closing;
        }
    }
}
