// Purpose: 실제 CLI 세션의 build·조회·취소·종료와 frontend 표시 경계 검증.
// Pattern: Integration Test.
// Usage: cargo build 후 npm test in editors/vscode.
// Related: editors/vscode/src/client.ts, crates/kodstar-cli/src/session/mod.rs.

import { strict as assert } from "node:assert";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { SessionClient, SessionFailure } from "../src/client";
import { isRelationship } from "../src/protocol";

const executable =
    process.env.KODSTAR_TEST_BINARY ??
    resolve(
        __dirname,
        "../../../../target/debug",
        process.platform === "win32" ? "kodstar.exe" : "kodstar",
    );

test("adapter uses the same engine and closes its owned session", { timeout: 10000 }, async () => {
    const directory = await mkdtemp(join(tmpdir(), "kodstar-adapter-"));
    const root = join(directory, "project");
    const home = join(directory, "home");
    await mkdir(root);
    await mkdir(home);
    const client = new SessionClient(executable, root, {
        ...process.env,
        HOME: home,
        USERPROFILE: home,
    });
    try {
        await writeFile(join(root, "main.rs"), "fn target() {} fn caller() { target(); }");
        const build = await client.start("build", {}).promise;
        assert.equal(build.exitCode, 0);
        const query = await client.start("query", {
            query: { kind: "deps", file: "main.rs", name: "caller" },
            options: { refresh: "none" },
        }).promise;
        assert.equal(query.metadata?.freshness, "not_checked");
        assert.ok(query.rows.some((row) => isRelationship(row) && row.target?.name === "target"));
        const symbols = await client.start("query", { query: { kind: "symbols", file: "main.rs" } })
            .promise;
        assert.equal(symbols.rows.length, 2);
        await writeFile(
            join(root, "main.rs"),
            `fn target() {} fn caller() { ${"target();".repeat(20000)} }`,
        );
        const pending = client.start("build", {});
        pending.cancel();
        await assert.rejects(
            pending.promise,
            (error: unknown) => error instanceof SessionFailure && error.code === 130,
        );
    } finally {
        await client.close();
        await rm(directory, { recursive: true, force: true });
    }
});

test(
    "malformed completion rejects the request instead of leaving it pending",
    { timeout: 3000 },
    async () => {
        const directory = await mkdtemp(join(tmpdir(), "kodstar-invalid-session-"));
        await writeFile(
            join(directory, "serve"),
            `// Purpose: malformed session response fixture.
// Pattern: Test Fixture.
// Usage: client boundary regression test only.
// Related: editors/vscode/test/client.test.ts.
process.stdin.once('data', () => process.stdout.write('{"protocol_version":1,"id":"1","type":"complete"}\\n'));
`,
        );
        const client = new SessionClient(process.execPath, directory);
        try {
            await assert.rejects(client.start("languages", {}).promise, /completion.*exit code/);
        } finally {
            await client.close();
            await rm(directory, { recursive: true, force: true });
        }
    },
);
