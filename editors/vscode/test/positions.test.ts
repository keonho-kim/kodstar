// Purpose: 원본 바이트 열의 Unicode·CRLF·경계 변환 검증.
// Pattern: Unit Test.
// Usage: npm test in editors/vscode.
// Related: editors/vscode/src/positions.ts.

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { utf16Column } from "../src/positions";

test("byte columns preserve Korean characters and surrogate pairs", () => {
    assert.equal(utf16Column("가😀call()\r", 8), 3);
    assert.equal(utf16Column("가😀call()\r", 1), 0);
    assert.equal(utf16Column("  call()", 3), 2);
    assert.throws(() => utf16Column("가call()", 2));
    assert.throws(() => utf16Column("abc", 0));
    assert.throws(() => utf16Column("abc", 5));
});
