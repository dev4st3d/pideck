import { closeSync, fstatSync, openSync, readSync } from "node:fs";

export const MAX_TRANSCRIPT_ENTRIES = 80;
export const MAX_TRANSCRIPT_BYTES = 48 * 1024;
const MAX_TRANSCRIPT_SOURCE_ENTRIES = MAX_TRANSCRIPT_ENTRIES * 2;
const MAX_TRANSCRIPT_FILE_READ_BYTES = 256 * 1024;

export function reconnectDelay(attempt, minimumMs = 250, maximumMs = 5_000) {
  const normalizedAttempt = Number.isFinite(attempt) ? Math.max(0, Math.floor(attempt)) : 0;
  const minimum = Math.max(1, Math.floor(minimumMs));
  const maximum = Math.max(minimum, Math.floor(maximumMs));
  return Math.min(maximum, minimum * (2 ** Math.min(normalizedAttempt, 20)));
}

export function isLiveSubagentStatus(status) {
  return status === "running" || status === "queued";
}

export function subagentTaskOutcome(data, failed = false) {
  const status = String(data?.status ?? (failed ? "error" : "completed"));
  // pi-subagents emits both completed and steered terminal records on its
  // successful completion channel. "steered" means the agent wrapped up at
  // its turn boundary, not that the task failed.
  const succeeded = !failed && (status === "completed" || status === "steered");
  return {
    succeeded,
    status,
    result: succeeded ? String(data?.result ?? "") : undefined,
    error: succeeded
      ? undefined
      : String(data?.error ?? (status === "stopped" ? "Stopped by user." : status)),
  };
}

export function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function taskRuntimeMetadata(metadata, { keepAgentId = false } = {}) {
  const source = isRecord(metadata) ? metadata : {};
  const { agentId, result: _result, lastError: _lastError, ...stable } = source;
  return {
    ...stable,
    // pi-tasks shallow-merges metadata and treats null as a deletion tombstone.
    agentId: keepAgentId && typeof agentId === "string" && agentId ? agentId : null,
    result: null,
    lastError: null,
  };
}

export function taskOpenBlockers(task, tasks) {
  const byId = new Map(tasks.map((candidate) => [String(candidate.id), candidate]));
  return (Array.isArray(task.blockedBy) ? task.blockedBy : []).filter((id) => {
    const blocker = byId.get(String(id));
    return !blocker || blocker.status !== "completed";
  });
}

export function taskCycleMembers(tasks) {
  const byId = new Map(tasks.map((task) => [String(task.id), task]));
  const visiting = [];
  const visited = new Set();
  const cycles = new Set();

  const visit = (id) => {
    const cycleStart = visiting.indexOf(id);
    if (cycleStart >= 0) {
      for (const member of visiting.slice(cycleStart)) cycles.add(member);
      return;
    }
    if (visited.has(id)) return;
    visited.add(id);
    visiting.push(id);
    const task = byId.get(id);
    for (const blocker of Array.isArray(task?.blockedBy) ? task.blockedBy : []) {
      visit(String(blocker));
    }
    visiting.pop();
  };

  for (const id of byId.keys()) visit(id);
  return cycles;
}

export function cascadeReadyTasks(tasks, completedId) {
  return tasks.filter(
    (task) =>
      task.status === "pending" &&
      Array.isArray(task.blockedBy) &&
      task.blockedBy.map(String).includes(String(completedId)) &&
      taskOpenBlockers(task, tasks).length === 0,
  );
}

export function normalizeSchedules(data) {
  return (Array.isArray(data?.jobs) ? data.jobs : []).map((job) => ({
    id: String(job.id ?? ""),
    name: String(job.name ?? job.description ?? "Scheduled agent"),
    description: String(job.description ?? ""),
    schedule: String(job.schedule ?? ""),
    scheduleType: String(job.scheduleType ?? "once"),
    subagentType: String(job.subagent_type ?? "general-purpose"),
    enabled: job.enabled !== false,
    createdAt: String(job.createdAt ?? ""),
    lastRun: typeof job.lastRun === "string" ? job.lastRun : undefined,
    lastStatus: typeof job.lastStatus === "string" ? job.lastStatus : undefined,
    nextRun: typeof job.nextRun === "string" ? job.nextRun : undefined,
    runCount: Number.isFinite(job.runCount) ? Math.max(0, Math.floor(job.runCount)) : 0,
  }));
}

export function agentQueuePositions(records) {
  const queued = (Array.isArray(records) ? records : []).filter(
    (record) => record?.status === "queued",
  );
  return new Map(queued.map((record, index) => [String(record.id), index + 1]));
}

export function latestGoalState(entries, rawSettings = {}) {
  const state = [...(Array.isArray(entries) ? entries : [])]
    .reverse()
    .find((entry) => entry?.type === "custom" && entry.customType === "goal-state")?.data;
  if (!isRecord(state)) return undefined;
  const queue = Array.isArray(state.queue) ? state.queue.filter(isRecord).map(normalizeGoal) : [];
  const pendingAction = isRecord(state.pendingAction) ? state.pendingAction : undefined;
  if (!isRecord(state.goal) && queue.length === 0 && !pendingAction) return undefined;
  const active = isRecord(state.goal) ? normalizeGoal(state.goal) : undefined;
  const settings = normalizeGoalSettings(rawSettings);
  const hasOrderedState = active?.status === "queued" || queue.length > 0 || pendingAction !== undefined;
  return {
    active,
    queue,
    pendingAction,
    queueFrozen: hasOrderedState && !settings.experimentalGoals,
    automaticTurnLimit: settings.automaticTurnLimit,
    noProgressTurnLimit: settings.noProgressTurnLimit,
  };
}

export function normalizeGoalSettings(value) {
  const defaults = {
    experimentalGoals: false,
    automaticTurnLimit: undefined,
    noProgressTurnLimit: 3,
  };
  if (!isRecord(value)) return defaults;

  const toolVisibility = value.toolVisibility ?? "always";
  if (toolVisibility !== "always" && toolVisibility !== "after-first-goal") return defaults;
  const experimental = value.experimental ?? {};
  if (!isRecord(experimental) || (experimental.goals !== undefined && typeof experimental.goals !== "boolean")) {
    return defaults;
  }
  const limits = value.continuationLimits ?? {};
  if (!isRecord(limits)) return defaults;
  const automaticTurnLimit = continuationLimit(limits.automaticTurns, undefined);
  const noProgressTurnLimit = continuationLimit(limits.noProgressTurns, 3);
  if (automaticTurnLimit === null || noProgressTurnLimit === null) return defaults;

  return {
    experimentalGoals: experimental.goals === true,
    automaticTurnLimit: automaticTurnLimit.value,
    noProgressTurnLimit: noProgressTurnLimit.value,
  };
}

function continuationLimit(value, fallback) {
  if (value === undefined) return { value: fallback };
  if (value === null) return { value: undefined };
  if (Number.isSafeInteger(value) && value > 0) return { value };
  return null;
}

function normalizeGoal(goal) {
  const activeStartedAt = Number.isFinite(goal.activeStartedAt)
    ? finiteNumber(goal.activeStartedAt)
    : undefined;
  const activeElapsed =
    goal.status === "active" && activeStartedAt !== undefined
      ? Math.max(0, Date.now() - activeStartedAt) / 1000
      : 0;
  const safetyPauseCause =
    goal.safetyPauseCause === "continuation_limit" || goal.safetyPauseCause === "no_progress"
      ? goal.safetyPauseCause
      : undefined;
  return {
    id: String(goal.id ?? ""),
    objective: String(goal.text ?? ""),
    status: String(goal.status ?? "paused"),
    startedAt: finiteNumber(goal.startedAt),
    updatedAt: finiteNumber(goal.updatedAt),
    iteration: finiteNumber(goal.iteration),
    tokenBudget: Number.isFinite(goal.tokenBudget) ? Math.max(0, Math.floor(goal.tokenBudget)) : undefined,
    tokensUsed: finiteNumber(goal.tokensUsed),
    timeUsedSeconds: finiteNumber(goal.timeUsedSeconds) + activeElapsed,
    activeStartedAt,
    automaticModelTurns: safeCounter(goal.automaticModelTurns),
    toolFreeRepeatCount: safeCounter(goal.toolFreeRepeatCount),
    safetyPauseCause,
  };
}

function safeCounter(value) {
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

function finiteNumber(value) {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

function contentText(content) {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((part) => {
      if (typeof part === "string") return part;
      if (part?.type === "text" || part?.type === "thinking") return String(part.text ?? "");
      if (part?.type === "toolCall") {
        return `${part.name ?? "tool"} ${JSON.stringify(part.arguments ?? {})}`;
      }
      return "";
    })
    .filter(Boolean)
    .join("\n");
}

export function transcriptEntry(message, timestamp) {
  if (!isRecord(message)) return undefined;
  const role =
    message.role === "user"
      ? "user"
      : message.role === "assistant"
        ? "assistant"
        : message.role === "toolResult"
          ? "tool_result"
          : "system";
  const rawContent = contentText(message.content);
  const content =
    rawContent.length > MAX_TRANSCRIPT_BYTES
      ? `…${rawContent.slice(-MAX_TRANSCRIPT_BYTES)}`
      : rawContent;
  if (!content && role === "system") return undefined;
  return {
    role,
    content,
    timestamp: typeof timestamp === "string" ? timestamp : undefined,
    toolName: typeof message.toolName === "string" ? message.toolName : undefined,
    isError: message.isError === true || message.stopReason === "error",
  };
}

export function transcriptFromMessages(messages) {
  const entries = [];
  const source = Array.isArray(messages) ? messages : [];
  const start = Math.max(0, source.length - MAX_TRANSCRIPT_SOURCE_ENTRIES);
  for (const message of source.slice(start)) {
    const entry = transcriptEntry(message);
    if (entry) entries.push(entry);
  }
  const bounded = boundTranscript(entries);
  bounded.truncated ||= start > 0;
  return bounded;
}

export function transcriptFromFile(path) {
  if (typeof path !== "string" || !path) return { entries: [], truncated: false };
  let descriptor;
  let text = "";
  let sourceTruncated = false;
  try {
    descriptor = openSync(path, "r");
    const size = fstatSync(descriptor).size;
    const start = Math.max(0, size - MAX_TRANSCRIPT_FILE_READ_BYTES);
    const bytes = Buffer.allocUnsafe(size - start);
    const read = readSync(descriptor, bytes, 0, bytes.length, start);
    text = bytes.subarray(0, read).toString("utf8");
    sourceTruncated = start > 0;
    if (sourceTruncated) {
      const firstRecordEnd = text.indexOf("\n");
      text = firstRecordEnd >= 0 ? text.slice(firstRecordEnd + 1) : "";
    }
  } catch {
    return { entries: [], truncated: false };
  } finally {
    if (descriptor !== undefined) {
      try {
        closeSync(descriptor);
      } catch {
        // The read result is already bounded and usable.
      }
    }
  }
  const entries = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    try {
      const parsed = JSON.parse(line);
      const entry = transcriptEntry(parsed.message, parsed.timestamp);
      if (entry) entries.push(entry);
    } catch {
      // A concurrently appended trailing record may be incomplete.
    }
  }
  const bounded = boundTranscript(entries);
  bounded.truncated ||= sourceTruncated;
  return bounded;
}

export function fitSnapshotRecord(snapshot, maxBytes) {
  const fitted = {
    ...snapshot,
    tasks: (snapshot.tasks ?? []).map((task) => ({
      ...task,
      subject: boundedText(task.subject, 1024),
      description: boundedText(task.description, 4096),
      output: boundedOptionalText(task.output, 8192),
      metadata: boundedMetadata(task.metadata),
    })),
    subagents: (snapshot.subagents ?? []).map((agent) => ({
      ...agent,
      description: boundedText(agent.description, 2048),
      result: boundedOptionalText(agent.result, 16 * 1024),
      error: boundedOptionalText(agent.error, 4096),
      transcript: [...(agent.transcript ?? [])],
    })),
    diagnostics: (snapshot.diagnostics ?? []).map((message) => boundedText(message, 2048)),
  };

  const encode = () => JSON.stringify({ type: "snapshot", snapshot: fitted });
  let encoded = encode();
  for (let index = fitted.subagents.length - 1; encodedBytes(encoded) > maxBytes && index >= 0; index--) {
    const agent = fitted.subagents[index];
    if (agent.transcript.length === 0) continue;
    agent.transcript = [];
    agent.transcriptTruncated = true;
    encoded = encode();
  }
  if (encodedBytes(encoded) > maxBytes) {
    for (const task of fitted.tasks) {
      task.output = undefined;
      task.metadata = {};
    }
    encoded = encode();
  }
  if (encodedBytes(encoded) > maxBytes) {
    for (const agent of fitted.subagents) agent.result = undefined;
    encoded = encode();
  }
  while (encodedBytes(encoded) > maxBytes) {
    const index = fitted.subagents.findLastIndex(
      (agent) => agent.status !== "running" && agent.status !== "queued",
    );
    if (index < 0) break;
    fitted.subagents.splice(index, 1);
    encoded = encode();
  }
  while (encodedBytes(encoded) > maxBytes) {
    const index = fitted.tasks.findLastIndex((task) => task.status === "completed");
    if (index < 0) break;
    fitted.tasks.splice(index, 1);
    encoded = encode();
  }
  if (encodedBytes(encoded) > maxBytes) {
    fitted.schedules = [];
    fitted.diagnostics = ["Some orchestration details were omitted to keep the connection responsive."];
    encoded = encode();
  }
  if (encodedBytes(encoded) > maxBytes) {
    fitted.tasks = fitted.tasks.filter((task) => task.status === "in_progress").slice(0, 32);
    fitted.subagents = fitted.subagents
      .filter((agent) => agent.status === "running" || agent.status === "queued")
      .slice(0, 32);
    encoded = encode();
  }
  if (encodedBytes(encoded) > maxBytes) {
    fitted.tasks = [];
    fitted.subagents = [];
    fitted.goal = undefined;
    encoded = encode();
  }
  return { snapshot: fitted, encoded };
}

function boundTranscript(entries) {
  let bytes = 0;
  const selected = [];
  for (let index = entries.length - 1; index >= 0; index--) {
    const entry = entries[index];
    const size = Buffer.byteLength(JSON.stringify(entry), "utf8");
    if (selected.length >= MAX_TRANSCRIPT_ENTRIES || bytes + size > MAX_TRANSCRIPT_BYTES) break;
    selected.push(entry);
    bytes += size;
  }
  selected.reverse();
  return { entries: selected, truncated: selected.length < entries.length };
}

function boundedText(value, maxCharacters) {
  const text = String(value ?? "");
  return text.length <= maxCharacters ? text : `${text.slice(0, maxCharacters)}…`;
}

function boundedOptionalText(value, maxCharacters) {
  return typeof value === "string" ? boundedText(value, maxCharacters) : undefined;
}

function boundedMetadata(metadata) {
  if (!isRecord(metadata)) return {};
  const encoded = JSON.stringify(metadata);
  if (encoded.length <= 4096) return metadata;
  return {
    ...(typeof metadata.agentId === "string" ? { agentId: metadata.agentId } : {}),
    ...(typeof metadata.agentType === "string" ? { agentType: metadata.agentType } : {}),
    ...(typeof metadata.lastError === "string"
      ? { lastError: boundedText(metadata.lastError, 2048) }
      : {}),
  };
}

function encodedBytes(value) {
  return Buffer.byteLength(value, "utf8");
}

export function goalCommand(action) {
  switch (action.kind) {
    case "goal_pause":
      return "/goal pause";
    case "goal_resume":
      return "/goal resume";
    case "goal_clear":
      return "/goal clear";
    case "goal_edit": {
      const objective = String(action.objective ?? "").trim();
      if (!objective) throw new Error("A goal objective is required.");
      const escaped = objective.replaceAll("\\", "\\\\").replaceAll('"', '\\"');
      const budget = Number.isSafeInteger(action.tokenBudget) && action.tokenBudget > 0
        ? ` --tokens ${action.tokenBudget}`
        : "";
      return `/goal edit${budget} "${escaped}"`;
    }
    default:
      return undefined;
  }
}
