// Purpose: 소유 CLI 세션·제한된 요청·표시 버퍼와 취소·종료 관리.
// Pattern: Process Adapter.
// Usage: VS Code 명령이 start를 호출하고 deactivate가 close를 기다림.
// Related: editors/vscode/src/protocol.ts, editors/vscode/src/commands.ts, editors/vscode/test/client.test.ts.

import { spawn, ChildProcessWithoutNullStreams } from "node:child_process";
import { decode, object, Metadata, Result, Row, Summary } from "./protocol";

const MAX_FRAME = 2 * 1024 * 1024;
const MAX_ROWS = 1000;
const MAX_RESULT_BYTES = 8 * 1024 * 1024;
interface Pending {
    result: Result;
    bytes: number;
    limited: boolean;
    error?: string;
    resolve(value: Result): void;
    reject(error: Error): void;
    progress?: (value: Row) => void;
}
export interface RequestHandle {
    promise: Promise<Result>;
    cancel(): void;
}
export class SessionFailure extends Error {
    constructor(
        public readonly code: number,
        message: string,
        public readonly result: Result,
    ) {
        super(message);
    }
}

export class SessionClient {
    private readonly child: ChildProcessWithoutNullStreams;
    private readonly exit: Promise<void>;
    private readonly pending = new Map<string, Pending>();
    private sequence = 0n;
    private buffer = "";
    private stderr = "";
    private failure?: Error;
    private ended = false;
    private closing?: Promise<void>;

    constructor(executable: string, cwd: string, env: NodeJS.ProcessEnv = process.env) {
        this.child = spawn(executable, ["serve", "--stdio"], {
            cwd,
            env,
            shell: false,
            windowsHide: true,
            stdio: "pipe",
        });
        this.child.stdout.setEncoding("utf8");
        this.child.stderr.setEncoding("utf8");
        this.child.stdout.on("data", (chunk: string) => this.receive(chunk));
        this.child.stderr.on("data", (chunk: string) => {
            this.stderr = (this.stderr + chunk).slice(-4096);
        });
        this.child.on("error", (error) =>
            this.fail(
                "code" in error && error.code === "ENOENT"
                    ? new Error(
                          "KodStar executable was not found. Build it and configure kodstar.executable on this workspace host",
                          { cause: error },
                      )
                    : error,
            ),
        );
        this.child.stdin.on("error", (error) => this.fail(error));
        this.exit = new Promise((resolve) =>
            this.child.once("close", (code, signal) => {
                this.ended = true;
                if (this.pending.size)
                    this.fail(
                        new Error(
                            `KodStar session closed before completion (${signal ?? code}): ${this.stderr}`,
                        ),
                    );
                resolve();
            }),
        );
    }

    get closed(): boolean {
        return this.ended || this.failure !== undefined || this.closing !== undefined;
    }

    start(method: string, params: unknown, progress?: (value: Row) => void): RequestHandle {
        if (this.closed) throw this.failure ?? new Error("KodStar session is closed");
        if (this.pending.size >= 3) throw new Error("KodStar already has three pending requests");
        const id = (++this.sequence).toString();
        const frame = JSON.stringify({ protocol_version: 1, id, method, params }) + "\n";
        if (Buffer.byteLength(frame, "utf8") > 65536)
            throw new Error("KodStar request exceeds 64 KiB");
        const promise = new Promise<Result>((resolve, reject) => {
            this.pending.set(id, {
                result: { rows: [], exitCode: 0 },
                bytes: 0,
                limited: false,
                resolve,
                reject,
                progress,
            });
        });
        this.child.stdin.write(frame, (error) => {
            if (error) this.fail(error);
        });
        let requested = false;
        return {
            promise,
            cancel: () => {
                if (!requested) {
                    requested = true;
                    this.cancel(id);
                }
            },
        };
    }

    private control(method: string, params?: unknown): void {
        if (!this.ended && this.child.stdin.writable) {
            const id = `control-${++this.sequence}`;
            this.child.stdin.write(
                JSON.stringify({ protocol_version: 1, id, method, params }) + "\n",
                (error) => {
                    if (error) this.fail(error);
                },
            );
        }
    }
    private cancel(id: string): void {
        if (this.pending.has(id)) this.control("cancel", { request_id: id });
    }

    private receive(chunk: string): void {
        if (this.failure) return;
        this.buffer += chunk;
        try {
            let newline: number;
            while ((newline = this.buffer.indexOf("\n")) >= 0) {
                const line = this.buffer.slice(0, newline);
                this.buffer = this.buffer.slice(newline + 1);
                if (Buffer.byteLength(line, "utf8") > MAX_FRAME)
                    throw new Error(
                        "KodStar frame exceeds the editor's 2 MiB limit; use the CLI for larger rows",
                    );
                if (line.trim()) this.event(line);
            }
            if (Buffer.byteLength(this.buffer, "utf8") > MAX_FRAME)
                throw new Error("KodStar frame exceeds the editor's 2 MiB limit");
        } catch (error) {
            this.fail(error instanceof Error ? error : new Error(String(error)));
        }
    }

    private event(line: string): void {
        const event = decode(line);
        if (event.id === null)
            throw new Error(event.message ?? "KodStar rejected an unidentifiable request");
        const id = String(event.id);
        const pending = this.pending.get(id);
        if (!pending) return;
        const data = object(event.data);
        switch (event.type) {
            case "metadata":
                if (!data) throw new Error("Invalid KodStar metadata");
                pending.result.metadata = data as unknown as Metadata;
                break;
            case "progress":
                if (data) pending.progress?.(data);
                break;
            case "result":
                if (!data) throw new Error("Invalid KodStar result row");
                if (pending.limited) break;
                pending.bytes += Buffer.byteLength(line, "utf8");
                if (pending.result.rows.length >= MAX_ROWS || pending.bytes > MAX_RESULT_BYTES) {
                    pending.limited = true;
                    this.cancel(id);
                } else pending.result.rows.push(data);
                break;
            case "summary":
                if (data) pending.result.summary = data as unknown as Summary;
                break;
            case "error":
                pending.error = event.message;
                break;
            case "complete": {
                const code = event.exit_code;
                if (typeof code !== "number")
                    throw new Error("KodStar completion lacks an exit code");
                pending.result.exitCode = pending.limited ? 3 : code;
                if (pending.limited) {
                    pending.result.summary = {
                        ...pending.result.summary,
                        complete: false,
                        truncated: true,
                        result_count: pending.result.rows.length,
                        unresolved_count: pending.result.metadata?.unresolved_count ?? 0,
                        reasons: [
                            ...(pending.result.summary?.reasons ?? []),
                            "editor_display_limit",
                        ],
                    };
                }
                this.pending.delete(id);
                if (code === 0 || code === 3 || pending.limited) pending.resolve(pending.result);
                else
                    pending.reject(
                        new SessionFailure(
                            code,
                            pending.error ?? `KodStar request failed (${code})`,
                            pending.result,
                        ),
                    );
                break;
            }
        }
    }

    private fail(error: Error): void {
        if (this.failure) return;
        this.failure = error;
        for (const pending of this.pending.values()) pending.reject(error);
        this.pending.clear();
        this.buffer = "";
        this.child.stdin.end();
        this.child.kill();
    }

    close(): Promise<void> {
        return (this.closing ??= this.finish());
    }
    private async finish(): Promise<void> {
        if (!this.ended && !this.failure) {
            this.control("shutdown");
            this.child.stdin.end();
        }
        let timer: NodeJS.Timeout | undefined;
        const exited = await Promise.race([
            this.exit.then(() => true),
            new Promise<false>((resolve) => {
                timer = setTimeout(() => resolve(false), 2000);
            }),
        ]);
        if (timer) clearTimeout(timer);
        if (!exited) this.child.kill("SIGKILL");
        await this.exit;
    }
}
