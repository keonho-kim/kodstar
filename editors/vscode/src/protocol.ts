// Purpose: 세션 envelope와 에디터가 사용하는 구조화된 결과 값.
// Pattern: Boundary Parser.
// Usage: client가 frame을 해석하고 view·명령이 결과를 표시.
// Related: editors/vscode/src/client.ts, editors/vscode/src/view.ts, crates/kodstar-cli/src/session/output.rs.

export type Row = Record<string, unknown>;
export interface Span {
    line: number;
    column: number;
    start_byte: number;
    end_byte: number;
}
export interface SymbolRow extends Row {
    id: number;
    logical_key: string;
    name: string;
    qualified_name: string;
    kind: string;
    file: string;
    definition: Span;
    file_encoding?: unknown;
}
export interface Relationship extends Row {
    source: SymbolRow;
    target: SymbolRow | null;
    external: string | null;
    relation: string;
    direction: string;
    resolution: string;
    rule: string;
    reason: string | null;
    site: { file: string; span: Span; path_encoding?: unknown };
}
export interface Metadata {
    relation_registry_version: number;
    root: string | null;
    root_encoding?: unknown;
    generation: number | null;
    freshness: string;
    unresolved_count: number;
    coverage: { analyzed_files: number; skipped_files: number; unsupported_files: number };
}
export interface Summary {
    complete: boolean;
    truncated: boolean;
    result_count: number;
    unresolved_count: number;
    reasons: string[];
}
export interface Result {
    rows: Row[];
    metadata?: Metadata;
    summary?: Summary;
    exitCode: number;
}
export interface Event {
    protocol_version: number;
    id: string | number | null;
    type: string;
    data?: unknown;
    code?: number;
    message?: string;
    exit_code?: number;
}
export function object(value: unknown): Row | undefined {
    return value !== null && typeof value === "object" && !Array.isArray(value)
        ? (value as Row)
        : undefined;
}
export function decode(line: string): Event {
    const frame = object(JSON.parse(line));
    if (
        !frame ||
        frame.protocol_version !== 1 ||
        typeof frame.type !== "string" ||
        !(frame.id === null || typeof frame.id === "string" || typeof frame.id === "number")
    ) {
        throw new Error("Unsupported KodStar session envelope");
    }
    return frame as unknown as Event;
}
export function isSymbol(row: Row): row is SymbolRow {
    return (
        typeof row.logical_key === "string" &&
        typeof row.qualified_name === "string" &&
        typeof row.file === "string" &&
        object(row.definition) !== undefined
    );
}
export function isRelationship(row: Row): row is Relationship {
    const source = object(row.source);
    return (
        source !== undefined &&
        isSymbol(source) &&
        typeof row.relation === "string" &&
        object(row.site) !== undefined
    );
}
