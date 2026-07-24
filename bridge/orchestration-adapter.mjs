import { readFileSync, statSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { createConnection } from "node:net";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

import {
  agentQueuePositions,
  cascadeReadyTasks,
  fitSnapshotRecord,
  goalCommand,
  latestGoalState,
  normalizeSchedules,
  taskCycleMembers,
  taskOpenBlockers,
  transcriptFromFile,
  transcriptFromMessages,
} from "./orchestration-core.mjs";

const ENDPOINT = process.env.PI_GUI_ORCHESTRATION_PIPE;
const AGENT_DIR =
  process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");
const MAX_RECORD_BYTES = 1024 * 1024;
const MAX_SNAPSHOT_RECORD_BYTES = 900 * 1024;
const MAX_TRANSCRIPT_FILE_CACHE = 64;
const MANAGER_KEY = Symbol.for("pi-subagents:manager");
const transcriptFileCache = new Map();

function readJson(path, fallback) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return fallback;
  }
}

function taskStorePath(ctx) {
  const override = process.env.PI_TASKS;
  if (override === "off") return undefined;
  if (override?.startsWith(".")) return resolve(ctx.cwd, override);
  if (override && isAbsolute(override)) return override;
  if (override) return override;
  const config = readJson(join(ctx.cwd, ".pi", "tasks-config.json"), {});
  if (config.taskScope === "memory") return undefined;
  if (config.taskScope === "project") return join(ctx.cwd, ".pi", "tasks", "tasks.json");
  return join(ctx.cwd, ".pi", "tasks", `tasks-${ctx.sessionManager.getSessionId()}.json`);
}

async function importInstalled(relativePath) {
  return import(pathToFileURL(join(AGENT_DIR, "npm", "node_modules", relativePath)).href);
}

let taskStoreModule;
async function openTaskStore(ctx) {
  const path = taskStorePath(ctx);
  if (!path) return undefined;
  taskStoreModule ??= await importInstalled("@tintinweb/pi-tasks/dist/task-store.js");
  return new taskStoreModule.TaskStore(path);
}

let agentTypesModule;
let agentRunnerModule;
let agentUsageModule;
async function agentMemory(type, cwd) {
  try {
    agentTypesModule ??= await importInstalled("@tintinweb/pi-subagents/dist/agent-types.js");
    const config = agentTypesModule.getAgentConfig(type);
    if (!config?.memory) return undefined;
    const path =
      config.memory === "user"
        ? join(homedir(), ".pi", "agent-memory", type)
        : config.memory === "local"
          ? join(cwd, ".pi", "agent-memory-local", type)
          : join(cwd, ".pi", "agent-memory", type);
    return { scope: config.memory, path };
  } catch {
    return undefined;
  }
}

function subagentSettings(cwd) {
  return {
    ...readJson(join(AGENT_DIR, "subagents.json"), {}),
    ...readJson(join(cwd, ".pi", "subagents.json"), {}),
  };
}

function scheduleSnapshots(ctx) {
  const sessionId = ctx.sessionManager.getSessionId();
  const data = readJson(
    join(ctx.cwd, ".pi", "subagent-schedules", `${sessionId}.json`),
    { jobs: [] },
  );
  return normalizeSchedules(data);
}

function taskOutput(task) {
  const result = task?.metadata?.result;
  if (typeof result === "string") return result;
  const error = task?.metadata?.lastError;
  return typeof error === "string" ? error : undefined;
}

async function taskSnapshots(ctx, diagnostics) {
  const store = await openTaskStore(ctx);
  if (!store) {
    diagnostics.push("The active pi-tasks store is memory-only and cannot survive a bridge restart.");
    return [];
  }
  const tasks = store.list();
  const cycles = taskCycleMembers(tasks);
  return tasks.map((task) => ({
    id: String(task.id),
    subject: String(task.subject ?? `Task ${task.id}`),
    description: String(task.description ?? ""),
    status: task.status,
    activeForm: typeof task.activeForm === "string" ? task.activeForm : undefined,
    owner: typeof task.owner === "string" ? task.owner : undefined,
    metadata: task.metadata && typeof task.metadata === "object" ? task.metadata : {},
    blocks: Array.isArray(task.blocks) ? task.blocks.map(String) : [],
    blockedBy: Array.isArray(task.blockedBy) ? task.blockedBy.map(String) : [],
    createdAt: Number.isFinite(task.createdAt) ? task.createdAt : 0,
    updatedAt: Number.isFinite(task.updatedAt) ? task.updatedAt : 0,
    output: taskOutput(task),
    cycle: cycles.has(String(task.id)),
  }));
}

function sessionEntries(ctx) {
  return ctx.sessionManager.getBranch?.() ?? ctx.sessionManager.getEntries?.() ?? [];
}

function restoredAgents(entries) {
  const records = new Map();
  for (let index = entries.length - 1; index >= 0; index--) {
    const entry = entries[index];
    if (entry?.type !== "custom" || entry.customType !== "subagents:record") continue;
    const id = String(entry.data?.id ?? "");
    if (id && !records.has(id)) records.set(id, entry.data);
  }
  return records;
}

function managerRegistry() {
  return globalThis[MANAGER_KEY];
}

function cachedTranscriptFromFile(path) {
  if (typeof path !== "string" || !path) return { entries: [], truncated: false };
  let fingerprint;
  try {
    const stat = statSync(path);
    fingerprint = `${stat.size}:${stat.mtimeMs}`;
  } catch {
    return { entries: [], truncated: false };
  }
  const cached = transcriptFileCache.get(path);
  if (cached?.fingerprint === fingerprint) return cached.transcript;
  const transcript = transcriptFromFile(path);
  transcriptFileCache.delete(path);
  transcriptFileCache.set(path, { fingerprint, transcript });
  while (transcriptFileCache.size > MAX_TRANSCRIPT_FILE_CACHE) {
    transcriptFileCache.delete(transcriptFileCache.keys().next().value);
  }
  return transcript;
}

async function subagentSnapshots(ctx, knownIds, entries) {
  const manager = managerRegistry();
  const settings = subagentSettings(ctx.cwd);
  const maxConcurrent = Number.isFinite(settings.maxConcurrent)
    ? Math.max(1, Math.floor(settings.maxConcurrent))
    : 4;
  const restored = restoredAgents(entries);
  const ids = new Set([...knownIds, ...restored.keys()]);
  const records = [...ids]
    .map((id) => manager?.getRecord?.(id) ?? restored.get(id))
    .filter(Boolean)
    .sort((left, right) => Number(right.startedAt ?? 0) - Number(left.startedAt ?? 0));
  const queuePositions = agentQueuePositions(records);
  return Promise.all(
    records.map(async (record) => {
      const liveTranscript = record.session?.messages
        ? transcriptFromMessages(record.session.messages)
        : undefined;
      const fileTranscript =
        liveTranscript?.entries.length > 0
          ? liveTranscript
          : cachedTranscriptFromFile(record.outputFile);
      return {
        id: String(record.id),
        type: String(record.type ?? "agent"),
        description: String(record.description ?? record.id),
        status: String(record.status ?? "error"),
        result: typeof record.result === "string" ? record.result : undefined,
        error: typeof record.error === "string" ? record.error : undefined,
        toolUses: Number.isFinite(record.toolUses) ? record.toolUses : 0,
        startedAt: Number.isFinite(record.startedAt) ? record.startedAt : 0,
        completedAt: Number.isFinite(record.completedAt) ? record.completedAt : undefined,
        queuePosition:
          record.status === "queued" ? queuePositions.get(String(record.id)) : undefined,
        maxConcurrent,
        outputFile: typeof record.outputFile === "string" ? record.outputFile : undefined,
        pendingSteers: Array.isArray(record.pendingSteers) ? record.pendingSteers.map(String) : [],
        worktree: record.worktree,
        worktreeResult: record.worktreeResult,
        memory: await agentMemory(String(record.type ?? "agent"), ctx.cwd),
        transcript: fileTranscript.entries,
        transcriptTruncated: fileTranscript.truncated,
      };
    }),
  );
}

function goalSnapshot(ctx, entries = sessionEntries(ctx)) {
  return latestGoalState(entries);
}

function actionError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

function rpcCall(pi, channel, params, timeoutMs = 10_000) {
  const requestId = randomUUID();
  return new Promise((resolveRequest, rejectRequest) => {
    const replyChannel = `${channel}:reply:${requestId}`;
    const timer = setTimeout(() => {
      unsubscribe();
      rejectRequest(actionError("timeout", `${channel} timed out.`));
    }, timeoutMs);
    const unsubscribe = pi.events.on(replyChannel, (reply) => {
      unsubscribe();
      clearTimeout(timer);
      if (reply?.success) resolveRequest(reply.data);
      else rejectRequest(actionError("rejected", String(reply?.error ?? "Action rejected.")));
    });
    pi.events.emit(channel, { requestId, ...params });
  });
}

function taskPrompt(task, tasks, additionalContext) {
  let prompt = `You are executing task #${task.id}: "${task.subject}"\n\n${task.description}`;
  const prerequisites = [];
  for (const dependencyId of task.blockedBy ?? []) {
    const dependency = tasks.find((candidate) => String(candidate.id) === String(dependencyId));
    if (typeof dependency?.metadata?.result === "string") {
      prerequisites.push(
        `### Task #${dependency.id}: ${dependency.subject}\n${dependency.metadata.result.slice(0, 4000)}`,
      );
    }
  }
  if (prerequisites.length) {
    prompt += `\n\n## Prerequisite task results\n\n${prerequisites.join("\n\n")}`;
  }
  if (additionalContext) prompt += `\n\n${additionalContext}`;
  return `${prompt}\n\nComplete this task fully. Do not attempt to manage tasks yourself.`;
}

export default function orchestrationAdapter(pi) {
  if (!ENDPOINT) return;

  let socket;
  let currentCtx;
  let reconnectTimer;
  let publishTimer;
  let pollTimer;
  let publishInFlight = false;
  let publishDirty = false;
  let publishForced = false;
  let snapshotWriteBlocked = false;
  let pendingSnapshotEncoded;
  let generation = 0;
  let lastSnapshotJson = "";
  const knownAgentIds = new Set();
  const taskAgentMap = new Map();
  const cascadeByTask = new Map();

  const send = (record) => {
    if (!socket || socket.destroyed) return false;
    const encoded = JSON.stringify(record);
    if (Buffer.byteLength(encoded, "utf8") > MAX_RECORD_BYTES) return false;
    socket.write(`${encoded}\n`);
    return true;
  };

  const writeSnapshot = (encoded) => {
    if (!socket || socket.destroyed) return false;
    if (snapshotWriteBlocked) {
      pendingSnapshotEncoded = encoded;
      return true;
    }
    snapshotWriteBlocked = !socket.write(`${encoded}\n`);
    return true;
  };

  const buildSnapshot = async () => {
    const ctx = currentCtx;
    if (!ctx) throw actionError("no_session", "No Pi session is active.");
    const diagnostics = [];
    const entries = sessionEntries(ctx);
    const [tasks, subagents] = await Promise.all([
      taskSnapshots(ctx, diagnostics),
      subagentSnapshots(ctx, knownAgentIds, entries),
    ]);
    if (currentCtx !== ctx) throw actionError("stale_session", "The Pi session changed.");
    return {
      sessionId: ctx.sessionManager.getSessionId(),
      generation: ++generation,
      capturedAt: Date.now(),
      tasks,
      subagents,
      schedules: scheduleSnapshots(ctx),
      goal: goalSnapshot(ctx, entries),
      diagnostics,
    };
  };

  const publishNow = async () => {
    if (publishInFlight) return;
    publishInFlight = true;
    try {
      do {
        publishDirty = false;
        const force = publishForced;
        publishForced = false;
        if (!currentCtx || !socket || socket.destroyed) continue;
        try {
          const built = await buildSnapshot();
          const { snapshot, encoded } = fitSnapshotRecord(built, MAX_SNAPSHOT_RECORD_BYTES);
          const comparable = JSON.stringify({ ...snapshot, generation: 0, capturedAt: 0 });
          if (!force && comparable === lastSnapshotJson) continue;
          lastSnapshotJson = comparable;
          if (Buffer.byteLength(encoded, "utf8") <= MAX_RECORD_BYTES) writeSnapshot(encoded);
        } catch {
          // A lifecycle edge can invalidate the context while the snapshot is building.
        }
      } while (publishDirty);
    } finally {
      publishInFlight = false;
      if (publishDirty) publishSoon();
    }
  };

  const publishSoon = (force = false) => {
    publishDirty = true;
    publishForced ||= force;
    if (publishTimer || publishInFlight) return;
    publishTimer = setTimeout(() => {
      publishTimer = undefined;
      void publishNow();
    }, 250);
  };

  const findTaskByAgent = async (agentId) => {
    if (!currentCtx) return undefined;
    const store = await openTaskStore(currentCtx);
    if (!store) return undefined;
    const mappedId = taskAgentMap.get(agentId);
    const task =
      (mappedId && store.get(mappedId)) ||
      store.list().find((candidate) => candidate.metadata?.agentId === agentId);
    return task ? { store, task } : undefined;
  };

  const executeTasks = async (action) => {
    const store = await openTaskStore(currentCtx);
    if (!store) throw actionError("store_unavailable", "The task store is memory-only.");
    const tasks = store.list();
    const cycles = taskCycleMembers(tasks);
    const launched = [];
    for (const taskId of action.taskIds ?? []) {
      const task = store.get(String(taskId));
      if (!task) throw actionError("stale_id", `Task ${taskId} no longer exists.`);
      if (task.status !== "pending") {
        throw actionError("guard", `Task ${taskId} is ${task.status}, not pending.`);
      }
      if (cycles.has(String(task.id))) {
        throw actionError("cycle", `Task ${taskId} belongs to a dependency cycle.`);
      }
      const blockers = taskOpenBlockers(task, tasks);
      if (blockers.length) {
        throw actionError("blocked", `Task ${taskId} is blocked by ${blockers.join(", ")}.`);
      }
      const agentType = task.metadata?.agentType;
      if (typeof agentType !== "string" || !agentType) {
        throw actionError("guard", `Task ${taskId} has no agentType.`);
      }
      store.update(task.id, { status: "in_progress" });
      try {
        const result = await rpcCall(
          pi,
          "subagents:rpc:spawn",
          {
            type: agentType,
            prompt: taskPrompt(task, tasks, action.additionalContext),
            options: {
              description: task.subject,
              isBackground: true,
              maxTurns: action.maxTurns,
              ...(action.model ? { model: action.model } : {}),
            },
          },
          30_000,
        );
        const agentId = String(result?.id ?? "");
        if (!agentId) throw actionError("spawn_failed", "Pi returned no subagent ID.");
        knownAgentIds.add(agentId);
        taskAgentMap.set(agentId, task.id);
        cascadeByTask.set(task.id, action);
        store.update(task.id, {
          owner: agentId,
          metadata: { ...task.metadata, agentId },
        });
        launched.push({ taskId: task.id, agentId });
      } catch (error) {
        store.update(task.id, {
          status: "pending",
          metadata: { ...task.metadata, lastError: error.message },
        });
        throw error;
      }
    }
    publishSoon(true);
    return { launched };
  };

  const completeTaskForAgent = async (data, failed) => {
    const found = await findTaskByAgent(String(data?.id ?? ""));
    if (!found) return;
    const { store, task } = found;
    taskAgentMap.delete(String(data.id));
    if (failed && data.status !== "stopped") {
      store.update(task.id, {
        status: "pending",
        metadata: { ...task.metadata, lastError: String(data.error ?? data.status ?? "failed") },
      });
      publishSoon(true);
      return;
    }
    store.update(task.id, {
      status: "completed",
      metadata: { ...task.metadata, result: String(data.result ?? task.metadata?.result ?? "") },
    });
    const cascade = cascadeByTask.get(task.id);
    if (cascade?.cascade) {
      const refreshed = store.list();
      const ready = cascadeReadyTasks(refreshed, task.id).filter(
        (candidate) => typeof candidate.metadata?.agentType === "string",
      );
      if (ready.length) {
        await executeTasks({ ...cascade, taskIds: ready.map((candidate) => candidate.id) });
      }
    }
    publishSoon(true);
  };

  const handleAction = async (action, sessionId) => {
    if (!currentCtx) throw actionError("no_session", "No Pi session is active.");
    const activeSessionId = currentCtx.sessionManager.getSessionId();
    if (sessionId && sessionId !== activeSessionId) {
      throw actionError("stale_session", "The selected Pi session has changed.");
    }
    if (action?.kind === "snapshot") {
      const snapshot = await buildSnapshot();
      return fitSnapshotRecord(snapshot, MAX_SNAPSHOT_RECORD_BYTES).snapshot;
    }
    switch (action?.kind) {
      case "task_execute":
        return executeTasks(action);
      case "task_stop": {
        const store = await openTaskStore(currentCtx);
        const task = store?.get(String(action.taskId ?? ""));
        if (!task) throw actionError("stale_id", "The task no longer exists.");
        if (task.status !== "in_progress" || typeof task.metadata?.agentId !== "string") {
          throw actionError("guard", "Only an in-progress subagent task can be stopped.");
        }
        await rpcCall(pi, "subagents:rpc:stop", { agentId: task.metadata.agentId });
        store.update(task.id, { status: "completed" });
        publishSoon(true);
        return { stopped: true };
      }
      case "subagent_stop": {
        const id = String(action.agentId ?? "");
        const record = managerRegistry()?.getRecord?.(id);
        if (!record) throw actionError("stale_id", "The subagent ID is stale.");
        if (record.status !== "running" && record.status !== "queued") {
          throw actionError("guard", `The subagent is ${record.status}.`);
        }
        await rpcCall(pi, "subagents:rpc:stop", { agentId: id });
        publishSoon(true);
        return { stopped: true };
      }
      case "subagent_steer": {
        const id = String(action.agentId ?? "");
        const message = String(action.message ?? "").trim();
        const record = managerRegistry()?.getRecord?.(id);
        if (!record) throw actionError("stale_id", "The subagent ID is stale.");
        if (record.status !== "running" && record.status !== "queued") {
          throw actionError("guard", `The subagent is ${record.status}.`);
        }
        if (!message) throw actionError("invalid", "A steering message is required.");
        if (record.session) await record.session.steer(message);
        else (record.pendingSteers ??= []).push(message);
        pi.events.emit("subagents:steered", { id, message });
        publishSoon(true);
        return { steered: true };
      }
      case "subagent_resume": {
        const id = String(action.agentId ?? "");
        const prompt = String(action.prompt ?? "").trim();
        const record = managerRegistry()?.getRecord?.(id);
        if (!record) throw actionError("stale_id", "The subagent ID is stale.");
        if (!record.session) throw actionError("guard", "The subagent has no resumable session.");
        if (record.status === "running" || record.status === "queued") {
          throw actionError("guard", "The subagent is already active.");
        }
        if (!prompt) throw actionError("invalid", "A resume prompt is required.");
        record.status = "running";
        record.startedAt = Date.now();
        record.completedAt = undefined;
        record.result = undefined;
        record.error = undefined;
        publishSoon(true);
        try {
          agentRunnerModule ??= await importInstalled(
            "@tintinweb/pi-subagents/dist/agent-runner.js",
          );
          agentUsageModule ??= await importInstalled(
            "@tintinweb/pi-subagents/dist/usage.js",
          );
          const { text, failure } = await agentRunnerModule.resumeAgent(record.session, prompt, {
            onToolActivity: (activity) => {
              if (activity?.type === "end") record.toolUses = (record.toolUses ?? 0) + 1;
            },
            onAssistantUsage: (usage) => {
              if (record.lifetimeUsage) agentUsageModule.addUsage(record.lifetimeUsage, usage);
            },
            onCompaction: (info) => {
              record.compactionCount = (record.compactionCount ?? 0) + 1;
              pi.events.emit("subagents:compacted", { id, ...info });
            },
          });
          record.result = text;
          record.status = failure ? "error" : "completed";
          record.error = failure;
        } catch (error) {
          record.status = "error";
          record.error = error instanceof Error ? error.message : String(error);
        }
        record.completedAt = Date.now();
        const payload = {
          id,
          type: record.type,
          description: record.description,
          status: record.status,
          result: record.result,
          error: record.error,
        };
        pi.events.emit(record.status === "error" ? "subagents:failed" : "subagents:completed", payload);
        pi.appendEntry("subagents:record", {
          ...payload,
          startedAt: record.startedAt,
          completedAt: record.completedAt,
        });
        publishSoon(true);
        return { resumed: true };
      }
      case "goal_pause":
      case "goal_resume":
      case "goal_edit":
      case "goal_clear": {
        const current = goalSnapshot(currentCtx);
        if (!current?.active || current.active.id !== String(action.goalId ?? "")) {
          throw actionError("stale_id", "The active goal has changed.");
        }
        return { invokeCommand: goalCommand(action) };
      }
      default:
        throw actionError("unsupported", "Unsupported orchestration action.");
    }
  };

  const attachSocket = (connected) => {
    socket = connected;
    snapshotWriteBlocked = false;
    pendingSnapshotEncoded = undefined;
    socket.setEncoding("utf8");
    const lines = createInterface({ input: socket, crlfDelay: Infinity });
    lines.on("line", (line) => {
      if (Buffer.byteLength(line, "utf8") > MAX_RECORD_BYTES) return;
      let request;
      try {
        request = JSON.parse(line);
      } catch {
        return;
      }
      if (request?.type !== "request" || typeof request.id !== "string") return;
      void handleAction(request.action, request.sessionId)
        .then((result) => send({ type: "response", id: request.id, ok: true, result }))
        .catch((error) =>
          send({
            type: "response",
            id: request.id,
            ok: false,
            error: {
              code: String(error?.code ?? "operation_failed"),
              message: String(error?.message ?? "Action failed.").slice(0, 240),
            },
          }),
        );
    });
    socket.on("close", () => {
      if (socket !== connected) return;
      socket = undefined;
      snapshotWriteBlocked = false;
      pendingSnapshotEncoded = undefined;
      clearInterval(pollTimer);
      pollTimer = undefined;
      scheduleReconnect();
    });
    socket.on("drain", () => {
      if (socket !== connected) return;
      snapshotWriteBlocked = false;
      const pending = pendingSnapshotEncoded;
      pendingSnapshotEncoded = undefined;
      if (pending) writeSnapshot(pending);
    });
    socket.on("error", () => {});
    pollTimer ??= setInterval(() => publishSoon(), 2000);
    pollTimer.unref?.();
    publishSoon(true);
  };

  const connect = () => {
    clearTimeout(reconnectTimer);
    reconnectTimer = undefined;
    const connected = createConnection(ENDPOINT);
    connected.once("connect", () => attachSocket(connected));
    connected.once("error", () => {
      connected.destroy();
      scheduleReconnect();
    });
  };

  const scheduleReconnect = () => {
    if (reconnectTimer) return;
    reconnectTimer = setTimeout(connect, 500);
    reconnectTimer.unref?.();
  };

  for (const eventName of [
    "subagents:created",
    "subagents:started",
    "subagents:completed",
    "subagents:failed",
    "subagents:steered",
    "subagents:compacted",
    "subagents:scheduler_ready",
  ]) {
    pi.events.on(eventName, (data) => {
      if (data?.id) knownAgentIds.add(String(data.id));
      if (eventName === "subagents:completed") void completeTaskForAgent(data, false);
      if (eventName === "subagents:failed") void completeTaskForAgent(data, true);
      publishSoon(true);
    });
  }

  pi.on("session_start", (_event, ctx) => {
    currentCtx = ctx;
    knownAgentIds.clear();
    taskAgentMap.clear();
    cascadeByTask.clear();
    transcriptFileCache.clear();
    lastSnapshotJson = "";
    publishSoon(true);
  });

  pi.on("session_info_changed", (_event, ctx) => {
    currentCtx = ctx;
    publishSoon(true);
  });

  pi.on("tool_execution_end", () => publishSoon());
  pi.on("session_shutdown", () => {
    currentCtx = undefined;
    clearInterval(pollTimer);
    pollTimer = undefined;
    send({ type: "snapshot", snapshot: undefined });
  });

  connect();
}
