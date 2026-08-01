import assert from "node:assert/strict";
import { once } from "node:events";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { test } from "node:test";

import { attachJsonlLineReader, serializeJsonLine } from "./jsonl.mjs";

async function waitFor(read, message, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = read();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(message);
}

async function connectWithRetry(endpoint, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    const socket = connect(endpoint);
    try {
      await Promise.race([
        once(socket, "connect"),
        once(socket, "error").then(([error]) => Promise.reject(error)),
      ]);
      return socket;
    } catch (error) {
      lastError = error;
      socket.destroy();
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }
  throw lastError ?? new Error("could not connect to Pi bridge");
}

test("orchestration reconnect grace hides quick adapter replacement", { timeout: 12_000 }, async () => {
  const root = mkdtempSync(join(tmpdir(), "pi-gui-bridge-"));
  const sdkRoot = join(root, "sdk");
  const endpoint =
    process.platform === "win32"
      ? `\\\\.\\pipe\\pi-gui-bridge-${process.pid}-${randomUUID()}`
      : join(root, "orchestration.sock");
  mkdirSync(join(sdkRoot, "dist"), { recursive: true });
  writeFileSync(join(sdkRoot, "package.json"), '{"version":"0.83.0","type":"module"}\n');
  writeFileSync(join(sdkRoot, "dist", "index.js"), "export {};\n");

  const child = spawn(
    process.execPath,
    [fileURLToPath(new URL("./pi-bridge.mjs", import.meta.url)), sdkRoot],
    {
      env: { ...process.env, PI_GUI_ORCHESTRATION_PIPE: endpoint },
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
  const records = [];
  let stderr = "";
  attachJsonlLineReader(child.stdout, (line) => records.push(JSON.parse(line)));
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });

  let first;
  let second;
  let third;
  let retired;
  try {
    first = await connectWithRetry(endpoint);
    first.write(serializeJsonLine({ type: "hello", producerId: "first" }));
    first.write(
      serializeJsonLine({
        type: "snapshot",
        snapshot: { sessionId: "session", producerId: "first", generation: 1, capturedAt: 1 },
      }),
    );
    await waitFor(
      () => records.find((record) => record.event === "orchestration_snapshot" && record.snapshot?.producerId === "first"),
      "first orchestration snapshot was not forwarded",
    );

    first.destroy();
    await new Promise((resolve) => setTimeout(resolve, 150));
    second = await connectWithRetry(endpoint);
    second.write(serializeJsonLine({ type: "hello", producerId: "second" }));
    second.write(
      serializeJsonLine({
        type: "snapshot",
        snapshot: { sessionId: "session", producerId: "second", generation: 1, capturedAt: 2 },
      }),
    );
    await waitFor(
      () => records.find((record) => record.event === "orchestration_snapshot" && record.snapshot?.producerId === "second"),
      "replacement orchestration snapshot was not forwarded",
    );

    const disconnectsBeforeGrace = records.filter(
      (record) => record.event === "orchestration_disconnected",
    ).length;
    await new Promise((resolve) => setTimeout(resolve, 2_700));
    assert.equal(
      records.filter((record) => record.event === "orchestration_disconnected").length,
      disconnectsBeforeGrace,
    );

    const secondRequests = [];
    attachJsonlLineReader(second, (line) => secondRequests.push(JSON.parse(line)));
    child.stdin.write(
      serializeJsonLine({
        version: 1,
        type: "request",
        id: "pending-action",
        command: "orchestration_action",
        params: { action: { kind: "task_stop", taskId: "1" } },
      }),
    );
    await waitFor(
      () => secondRequests.find((record) => record.type === "request"),
      "bridge did not forward the pending orchestration action",
    );

    third = await connectWithRetry(endpoint);
    third.write(serializeJsonLine({ type: "hello", producerId: "third" }));
    third.write(
      serializeJsonLine({
        type: "snapshot",
        snapshot: { sessionId: "session", producerId: "third", generation: 1, capturedAt: 3 },
      }),
    );
    await waitFor(
      () => records.find((record) => record.event === "orchestration_snapshot" && record.snapshot?.producerId === "third"),
      "third orchestration snapshot was not forwarded",
    );
    const pendingFailure = await waitFor(
      () => records.find((record) => record.type === "response" && record.id === "pending-action"),
      "replacing the adapter did not fail its pending action",
    );
    assert.equal(pendingFailure.ok, false);

    const secondSnapshotCount = records.filter(
      (record) => record.event === "orchestration_snapshot" && record.snapshot?.producerId === "second",
    ).length;
    retired = await connectWithRetry(endpoint);
    retired.on("error", () => {});
    retired.write(serializeJsonLine({ type: "hello", producerId: "second" }));
    retired.write(
      serializeJsonLine({
        type: "snapshot",
        snapshot: { sessionId: "session", producerId: "second", generation: 2, capturedAt: 4 },
      }),
    );
    await new Promise((resolve) => retired.once("close", resolve));
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.equal(
      records.filter(
        (record) => record.event === "orchestration_snapshot" && record.snapshot?.producerId === "second",
      ).length,
      secondSnapshotCount,
    );

    third.destroy();
    await waitFor(
      () => records.some((record) => record.event === "orchestration_disconnected"),
      "sustained adapter loss was not surfaced",
      4_000,
    );
  } finally {
    first?.destroy();
    second?.destroy();
    third?.destroy();
    retired?.destroy();
    child.stdin.end();
    const exited = await Promise.race([
      once(child, "exit").then(() => true),
      new Promise((resolve) => setTimeout(() => resolve(false), 2_000)),
    ]);
    if (!exited) child.kill("SIGKILL");
    assert.equal(stderr, "");
    if (process.platform !== "win32") {
      try {
        unlinkSync(endpoint);
      } catch {
        // The bridge removes its endpoint during shutdown.
      }
    }
    rmSync(root, { recursive: true, force: true });
  }
});
