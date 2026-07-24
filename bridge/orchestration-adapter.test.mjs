import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { randomUUID } from "node:crypto";
import { test } from "node:test";

import { attachJsonlLineReader } from "./jsonl.mjs";

function eventBus() {
  const handlers = new Map();
  return {
    on(name, handler) {
      const listeners = handlers.get(name) ?? new Set();
      listeners.add(handler);
      handlers.set(name, listeners);
      return () => listeners.delete(handler);
    },
    emit(name, ...args) {
      for (const handler of handlers.get(name) ?? []) handler(...args);
    },
    async emitAsync(name, ...args) {
      await Promise.all(
        [...(handlers.get(name) ?? [])].map((handler) => Promise.resolve(handler(...args))),
      );
    },
  };
}

async function waitFor(read, message, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = read();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(message);
}

function writeFakeTaskStore(packageRoot) {
  mkdirSync(join(packageRoot, "dist"), { recursive: true });
  writeFileSync(join(packageRoot, "package.json"), '{"type":"module"}\n');
  writeFileSync(
    join(packageRoot, "dist", "task-store.js"),
    `import { readFileSync, writeFileSync } from "node:fs";
export class TaskStore {
  constructor(path) { this.path = path; }
  read() { return JSON.parse(readFileSync(this.path, "utf8")); }
  write(data) { writeFileSync(this.path, JSON.stringify(data)); }
  list() { return this.read().tasks; }
  get(id) { return this.read().tasks.find((task) => task.id === id); }
  update(id, fields) {
    const data = this.read();
    const task = data.tasks.find((candidate) => candidate.id === id);
    if (!task) return { task: undefined, changedFields: [], warnings: [] };
    for (const key of ["status", "subject", "description", "activeForm", "owner"]) {
      if (fields[key] !== undefined) task[key] = fields[key];
    }
    if (fields.metadata !== undefined) {
      for (const [key, value] of Object.entries(fields.metadata)) {
        if (value === null) delete task.metadata[key];
        else task.metadata[key] = value;
      }
    }
    task.updatedAt = Date.now();
    this.write(data);
    return { task, changedFields: [], warnings: [] };
  }
}
`,
  );
}

test("session lifecycle preserves live task agents and recovers orphaned runs", async () => {
  const root = mkdtempSync(join(tmpdir(), "pi-gui-orchestration-"));
  const project = join(root, "project");
  const agentDir = join(root, "agent");
  const taskPackage = join(
    agentDir,
    "npm",
    "node_modules",
    "@tintinweb",
    "pi-tasks",
  );
  const sessionId = "session-1";
  const taskPath = join(project, ".pi", "tasks", `tasks-${sessionId}.json`);
  const endpoint =
    process.platform === "win32"
      ? `\\\\.\\pipe\\pi-gui-orchestration-${process.pid}-${randomUUID()}`
      : join(root, "orchestration.sock");

  mkdirSync(dirname(taskPath), { recursive: true });
  writeFakeTaskStore(taskPackage);
  writeFileSync(
    taskPath,
    JSON.stringify({
      nextId: 3,
      tasks: [
        {
          id: "1",
          subject: "Live",
          description: "Still running",
          status: "in_progress",
          owner: "live-agent",
          metadata: { agentType: "worker", agentId: "live-agent", result: "stale" },
          blocks: [],
          blockedBy: [],
          createdAt: 1,
          updatedAt: 1,
        },
        {
          id: "2",
          subject: "Orphan",
          description: "Lost during restart",
          status: "in_progress",
          owner: "orphan-agent",
          metadata: {
            agentType: "worker",
            agentId: "orphan-agent",
            result: "stale",
            lastError: "old",
          },
          blocks: [],
          blockedBy: [],
          createdAt: 1,
          updatedAt: 1,
        },
      ],
    }),
  );

  const records = [];
  let connectedSocket;
  const server = createServer((socket) => {
    connectedSocket = socket;
    attachJsonlLineReader(socket, (line) => records.push(JSON.parse(line)));
  });

  const previousEndpoint = process.env.PI_GUI_ORCHESTRATION_PIPE;
  const previousAgentDir = process.env.PI_CODING_AGENT_DIR;
  const managerKey = Symbol.for("pi-subagents:manager");
  const previousManager = globalThis[managerKey];

  try {
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(endpoint, resolve);
    });
    process.env.PI_GUI_ORCHESTRATION_PIPE = endpoint;
    process.env.PI_CODING_AGENT_DIR = agentDir;
    globalThis[managerKey] = {
      getRecord(id) {
        return id === "live-agent" ? { id, status: "running" } : undefined;
      },
    };

    const lifecycle = eventBus();
    const events = eventBus();
    const pi = { events, on: lifecycle.on };
    const adapterUrl = new URL("./orchestration-adapter.mjs", import.meta.url);
    adapterUrl.searchParams.set("test", randomUUID());
    const { default: orchestrationAdapter } = await import(adapterUrl.href);
    orchestrationAdapter(pi);

    await waitFor(() => connectedSocket, "adapter did not connect");
    const ctx = {
      cwd: project,
      sessionManager: {
        getSessionId: () => sessionId,
        getBranch: () => [],
        getEntries: () => [],
      },
    };
    await lifecycle.emitAsync("session_start", {}, ctx);

    const snapshotRecord = await waitFor(
      () => records.find((record) => record.type === "snapshot" && record.snapshot),
      "adapter did not publish a session snapshot",
    );
    const afterStart = JSON.parse(readFileSync(taskPath, "utf8")).tasks;
    const live = afterStart.find((task) => task.id === "1");
    const orphan = afterStart.find((task) => task.id === "2");
    assert.equal(live.status, "in_progress");
    assert.equal(live.metadata.agentId, "live-agent");
    assert.equal(orphan.status, "pending");
    assert.equal(orphan.owner, "");
    assert.equal("agentId" in orphan.metadata, false);
    assert.equal("result" in orphan.metadata, false);
    assert.match(orphan.metadata.lastError, /restarted/i);
    assert.equal(
      snapshotRecord.snapshot.tasks.find((task) => task.id === "2").owner,
      undefined,
    );

    await lifecycle.emitAsync("session_shutdown");
    const afterShutdown = JSON.parse(readFileSync(taskPath, "utf8")).tasks;
    const resetLive = afterShutdown.find((task) => task.id === "1");
    assert.equal(resetLive.status, "pending");
    assert.equal(resetLive.owner, "");
    assert.equal("agentId" in resetLive.metadata, false);
    assert.match(resetLive.metadata.lastError, /session ended/i);
    await waitFor(
      () => records.some((record) => record.type === "snapshot" && record.snapshot === null),
      "adapter did not clear the session snapshot",
    );
  } finally {
    connectedSocket?.destroy();
    await new Promise((resolve) => server.close(resolve));
    if (process.platform !== "win32") {
      try {
        unlinkSync(endpoint);
      } catch {
        // The server may already have removed the socket.
      }
    }
    if (previousEndpoint === undefined) delete process.env.PI_GUI_ORCHESTRATION_PIPE;
    else process.env.PI_GUI_ORCHESTRATION_PIPE = previousEndpoint;
    if (previousAgentDir === undefined) delete process.env.PI_CODING_AGENT_DIR;
    else process.env.PI_CODING_AGENT_DIR = previousAgentDir;
    if (previousManager === undefined) delete globalThis[managerKey];
    else globalThis[managerKey] = previousManager;
    rmSync(root, { recursive: true, force: true });
  }
});
