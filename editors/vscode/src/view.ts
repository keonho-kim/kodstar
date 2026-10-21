// Purpose: 관계·정의·사용 위치와 인덱스 최신성·잘림 상태 표시.
// Pattern: Tree View Adapter.
// Usage: 명령 처리가 완성된 세션 결과를 view에 전달.
// Related: editors/vscode/src/commands.ts, editors/vscode/src/protocol.ts.

import * as vscode from "vscode";
import { isRelationship, isSymbol, Result, Row, Span, SymbolRow } from "./protocol";

export interface Location {
    root: string;
    file: string;
    span: Span;
    encoded: boolean;
}
class Entry extends vscode.TreeItem {
    parent?: Entry;
    constructor(
        label: string,
        description: string,
        readonly children: Entry[] = [],
        location?: Location,
    ) {
        super(
            label,
            children.length
                ? vscode.TreeItemCollapsibleState.Collapsed
                : vscode.TreeItemCollapsibleState.None,
        );
        this.description = description;
        for (const child of children) child.parent = this;
        if (location && !location.encoded) {
            this.command = {
                command: "kodstar.openLocation",
                title: "Open source location",
                arguments: [location],
            };
        } else if (location?.encoded)
            this.tooltip = "This path needs the CLI's lossless OS-path representation";
    }
}

export class RelationsView implements vscode.TreeDataProvider<Entry>, vscode.Disposable {
    private readonly changed = new vscode.EventEmitter<Entry | undefined>();
    readonly onDidChangeTreeData = this.changed.event;
    private entries: Entry[] = [];
    readonly tree: vscode.TreeView<Entry>;
    constructor() {
        this.tree = vscode.window.createTreeView("kodstar.relations", {
            treeDataProvider: this,
            showCollapseAll: true,
        });
    }
    getTreeItem(entry: Entry): vscode.TreeItem {
        return entry;
    }
    getChildren(entry?: Entry): Entry[] {
        return entry?.children ?? this.entries;
    }
    getParent(entry: Entry): Entry | undefined {
        return entry.parent;
    }
    reset(message: string): void {
        this.entries = [];
        this.message(message);
    }
    message(message: string): void {
        this.tree.message = message;
        this.changed.fire(undefined);
    }

    async show(result: Result, fallbackRoot: string): Promise<void> {
        const metadata = result.metadata;
        const root = metadata?.root ?? fallbackRoot;
        this.tree.title = `KodStar · ${root}`;
        const encoded = metadata?.root_encoding != null;
        this.entries = result.rows.map((row) => this.entry(row, root, encoded));
        const freshness =
            metadata?.freshness === "content_verified"
                ? "Content verified"
                : metadata?.freshness === "metadata_checked"
                  ? "Source metadata checked"
                  : "Freshness not checked";
        const coverage = metadata?.coverage;
        const partial = result.summary?.complete === false || result.exitCode !== 0;
        const reason = result.summary?.reasons.join(", ");
        this.message(
            `Generation ${metadata?.generation ?? "-"} · ${freshness} · ${partial ? "Incomplete" : "Complete for selected index"}` +
                ` · unresolved ${metadata?.unresolved_count ?? 0}` +
                (coverage
                    ? ` · skipped ${coverage.skipped_files} · unsupported ${coverage.unsupported_files}`
                    : "") +
                (reason ? ` · ${reason}` : ""),
        );
        if (this.entries[0]) await this.tree.reveal(this.entries[0], { focus: false });
    }

    private definition(
        symbol: SymbolRow,
        root: string,
        encoded: boolean,
        label = symbol.qualified_name,
    ): Entry {
        return new Entry(label, `${symbol.kind} · ${symbol.file}:${symbol.definition.line}`, [], {
            root,
            file: symbol.file,
            span: symbol.definition,
            encoded: encoded || symbol.file_encoding != null,
        });
    }
    private entry(row: Row, root: string, encoded: boolean): Entry {
        if (isSymbol(row)) return this.definition(row, root, encoded);
        if (isRelationship(row)) {
            const other = row.direction === "in" ? row.source : row.target;
            const children = [
                new Entry(
                    "Use",
                    `${row.site.file}:${row.site.span.line}:${row.site.span.column}`,
                    [],
                    {
                        root,
                        file: row.site.file,
                        span: row.site.span,
                        encoded: encoded || row.site.path_encoding != null,
                    },
                ),
            ];
            if (other) children.unshift(this.definition(other, root, encoded, "Definition"));
            const entry = new Entry(
                `${row.direction === "in" ? "←" : "→"} ${row.relation}: ${other?.qualified_name ?? row.external ?? "Unresolved"}`,
                `${row.resolution} · ${row.rule}`,
                children,
            );
            entry.tooltip = row.reason ?? `${row.resolution}: ${row.rule}`;
            return entry;
        }
        if (typeof row.code === "string") return new Entry(row.code, String(row.message ?? ""));
        if (typeof row.file === "string")
            return new Entry(row.file, `${row.direction ?? ""} ${row.relation ?? ""}`);
        if (typeof row.generation === "number")
            return new Entry(
                `Indexed generation ${row.generation}`,
                `parsed ${row.parsed_files ?? 0} files`,
            );
        return new Entry("Engine result", "Use the CLI for the full structured record");
    }
    dispose(): void {
        this.tree.dispose();
        this.changed.dispose();
    }
}
