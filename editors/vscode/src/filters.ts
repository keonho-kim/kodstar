// Purpose: 관계 방향·종류·깊이와 후보·외부 포함의 UI 입력.
// Pattern: Input Adapter.
// Usage: 명령 coordinator가 현재 필터를 전달하고 반환 값을 소유.
// Related: editors/vscode/src/commands.ts.

import * as vscode from "vscode";

export type Filters = {
    direction?: string;
    relations: string[];
    depth?: number;
    transitive?: boolean;
    include_candidates: boolean;
    include_external: boolean;
};
export async function editFilters(previous: Filters): Promise<Filters | undefined> {
    const direction = await vscode.window.showQuickPick(["command default", "in", "out", "both"], {
        placeHolder: "Relationship direction",
    });
    if (direction === undefined) return;
    const relations = await vscode.window.showInputBox({
        prompt: "Relation kinds separated by commas; empty uses engine defaults",
        value: previous.relations.join(","),
        placeHolder: "calls,uses_type",
    });
    if (relations === undefined) return;
    const depth = await vscode.window.showInputBox({
        prompt: "Depth: empty uses command default, 0 means transitive",
        value: previous.transitive ? "0" : (previous.depth?.toString() ?? ""),
        validateInput: (value) =>
            value === "" || (/^(0|[1-9][0-9]*)$/.test(value) && Number.isSafeInteger(Number(value)))
                ? undefined
                : "Enter a non-negative integer",
    });
    if (depth === undefined) return;
    const include = await vscode.window.showQuickPick(["candidates", "external"], {
        canPickMany: true,
        placeHolder: "Optional relationship states and external targets",
    });
    if (!include) return;
    return {
        direction: direction === "command default" ? undefined : direction,
        relations: relations
            .split(",")
            .map((value) => value.trim())
            .filter(Boolean),
        depth: depth && depth !== "0" ? Number(depth) : undefined,
        transitive: depth === "0",
        include_candidates: include.includes("candidates"),
        include_external: include.includes("external"),
    };
}
