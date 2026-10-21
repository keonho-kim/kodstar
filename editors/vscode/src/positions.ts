// Purpose: 원본 UTF-8 바이트 열을 에디터의 UTF-16 열로 변환.
// Pattern: Simple Module.
// Usage: 위치 이동 명령이 저장된 문서의 해당 줄을 전달.
// Related: editors/vscode/src/commands.ts, editors/vscode/test/positions.test.ts.

export function utf16Column(line: string, byteColumn: number): number {
    const bytes = Buffer.from(line, "utf8");
    if (!Number.isSafeInteger(byteColumn) || byteColumn < 1 || byteColumn > bytes.length + 1) {
        throw new Error("Byte column is outside the saved source line");
    }
    const prefix = bytes.subarray(0, byteColumn - 1);
    const text = prefix.toString("utf8");
    if (Buffer.byteLength(text, "utf8") !== prefix.length) {
        throw new Error("Byte column splits a UTF-8 character");
    }
    return text.length;
}
