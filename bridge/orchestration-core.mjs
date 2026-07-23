import { readFileSync } from "node:fs";

export const MAX_TRANSCRIPT_ENTRIES = 200;
export const MAX_TRANSCRIPT_BYTES = 512 * 1024;

export function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
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

export function latestGoalState(entries) {
  const state = [...(Array.isArray(entries) ? entries : [])]
    .reverse()
    .find((entry) => entry?.type === "custom" && entry.customType === "goal-state")?.data;
  if (!isRecord(state)) return undefined;
  const queue = Array.isArray(state.queue) ? state.queue.filter(isRecord).map(normalizeGoal) : [];
  const pendingAction = isRecord(state.pendingAction) ? state.pendingAction : undefined;
  if (!isRecord(state.goal) && queue.length === 0 && !pendingAction) return undefined;
  return {
    active: isRecord(state.goal) ? normalizeGoal(state.goal) : undefined,
    queue,
    pendingAction,
    queueFrozen: false,
  };
}

function normalizeGoal(goal) {
  const activeStartedAt = Number.isFinite(goal.activeStartedAt)
    ? finiteNumber(goal.activeStartedAt)
    : undefined;
  const activeElapsed =
    goal.status === "active" && activeStartedAt !== undefined
      ? Math.max(0, Date.now() - activeStartedAt) / 1000
      : 0;
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
  };
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
  const content = contentText(message.content);
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
  for (const message of Array.isArray(messages) ? messages : []) {
    const entry = transcriptEntry(message);
    if (entry) entries.push(entry);
  }
  return boundTranscript(entries);
}

export function transcriptFromFile(path) {
  if (typeof path !== "string" || !path) return { entries: [], truncated: false };
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch {
    return { entries: [], truncated: false };
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
  return boundTranscript(entries);
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
