import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MAX_TRANSCRIPT_ENTRIES,
  agentQueuePositions,
  cascadeReadyTasks,
  fitSnapshotRecord,
  goalCommand,
  isLiveSubagentStatus,
  latestGoalState,
  normalizeSchedules,
  reconnectDelay,
  subagentTaskOutcome,
  taskCycleMembers,
  taskOpenBlockers,
  taskRuntimeMetadata,
  transcriptFromMessages,
} from "./orchestration-core.mjs";

test("reconnect delay backs off quickly and remains bounded", () => {
  assert.deepEqual(
    [0, 1, 2, 3, 4, 5, 20].map((attempt) => reconnectDelay(attempt)),
    [250, 500, 1000, 2000, 4000, 5000, 5000],
  );
});

test("only successful subagent completion unlocks dependent tasks", () => {
  assert.deepEqual(subagentTaskOutcome({ status: "completed", result: "done" }), {
    succeeded: true,
    status: "completed",
    result: "done",
    error: undefined,
  });
  assert.deepEqual(subagentTaskOutcome({ status: "steered", result: "wrapped" }), {
    succeeded: true,
    status: "steered",
    result: "wrapped",
    error: undefined,
  });
  assert.deepEqual(subagentTaskOutcome({ status: "stopped" }), {
    succeeded: false,
    status: "stopped",
    result: undefined,
    error: "Stopped by user.",
  });
  assert.equal(subagentTaskOutcome({ status: "completed" }, true).succeeded, false);
  assert.equal(isLiveSubagentStatus("running"), true);
  assert.equal(isLiveSubagentStatus("queued"), true);
  assert.equal(isLiveSubagentStatus("completed"), false);
});

test("task runtime metadata clears stale execution state between retries", () => {
  const metadata = {
    agentType: "worker",
    agentId: "old-agent",
    result: "stale output",
    lastError: "stale error",
    custom: true,
  };
  assert.deepEqual(taskRuntimeMetadata(metadata), {
    agentType: "worker",
    custom: true,
    agentId: null,
    result: null,
    lastError: null,
  });
  assert.deepEqual(taskRuntimeMetadata(metadata, { keepAgentId: true }), {
    agentType: "worker",
    custom: true,
    agentId: "old-agent",
    result: null,
    lastError: null,
  });
});

test("task DAG guards blockers, detects cycles, and cascades only newly ready tasks", () => {
  const tasks = [
    { id: "1", status: "completed", blockedBy: [] },
    { id: "2", status: "pending", blockedBy: ["1"] },
    { id: "3", status: "pending", blockedBy: ["1", "2"] },
    { id: "4", status: "pending", blockedBy: ["missing"] },
  ];

  assert.deepEqual(taskOpenBlockers(tasks[1], tasks), []);
  assert.deepEqual(taskOpenBlockers(tasks[2], tasks), ["2"]);
  assert.deepEqual(taskOpenBlockers(tasks[3], tasks), ["missing"]);
  assert.deepEqual(cascadeReadyTasks(tasks, "1").map((task) => task.id), ["2"]);
  assert.deepEqual([...taskCycleMembers(tasks)], []);

  const cycle = [
    { id: "a", status: "pending", blockedBy: ["c"] },
    { id: "b", status: "pending", blockedBy: ["a"] },
    { id: "c", status: "pending", blockedBy: ["b"] },
  ];
  assert.deepEqual([...taskCycleMembers(cycle)].sort(), ["a", "b", "c"]);
});

test("concurrent agent records preserve queue order and terminal lifecycle states", () => {
  const records = [
    { id: "running", status: "running" },
    { id: "q1", status: "queued" },
    { id: "done", status: "completed", worktreeResult: { hasChanges: true } },
    { id: "q2", status: "queued" },
    { id: "failed", status: "error" },
    { id: "stopped", status: "stopped" },
  ];

  assert.deepEqual([...agentQueuePositions(records)], [
    ["q1", 1],
    ["q2", 2],
  ]);
  assert.equal(records[2].worktreeResult.hasChanges, true);
  assert.deepEqual(
    records.filter((record) => ["completed", "error", "stopped"].includes(record.status))
      .map((record) => record.id),
    ["done", "failed", "stopped"],
  );
});

test("schedule restoration normalizes persisted jobs without inventing state", () => {
  const schedules = normalizeSchedules({
    jobs: [
      {
        id: "nightly",
        description: "Review changes",
        schedule: "0 1 * * *",
        scheduleType: "cron",
        subagent_type: "review",
        enabled: false,
        runCount: 3,
        lastStatus: "completed",
      },
    ],
  });

  assert.deepEqual(schedules, [
    {
      id: "nightly",
      name: "Review changes",
      description: "Review changes",
      schedule: "0 1 * * *",
      scheduleType: "cron",
      subagentType: "review",
      enabled: false,
      createdAt: "",
      lastRun: undefined,
      lastStatus: "completed",
      nextRun: undefined,
      runCount: 3,
    },
  ]);
});

test("goal snapshots retain budget and elapsed state and actions stay command-backed", () => {
  const goal = latestGoalState([
    {
      type: "custom",
      customType: "goal-state",
      data: {
        goal: {
          id: "g1",
          text: "Ship Phase 16",
          status: "paused",
          tokenBudget: 5000,
          tokensUsed: 1250,
          timeUsedSeconds: 42.5,
          iteration: 2,
        },
        queue: [{ id: "g2", text: "Follow-up", status: "queued" }],
      },
    },
  ]);

  assert.equal(goal.active.id, "g1");
  assert.equal(goal.active.tokenBudget, 5000);
  assert.equal(goal.active.tokensUsed, 1250);
  assert.equal(goal.active.timeUsedSeconds, 42.5);
  assert.equal(goal.queue[0].objective, "Follow-up");
  assert.equal(goalCommand({ kind: "goal_pause" }), "/goal pause");
  assert.equal(goalCommand({ kind: "goal_resume" }), "/goal resume");
  assert.equal(
    goalCommand({
      kind: "goal_edit",
      objective: 'Ship "safely"',
      tokenBudget: 7000,
    }),
    '/goal edit --tokens 7000 "Ship \\"safely\\""',
  );
  assert.throws(
    () => goalCommand({ kind: "goal_edit", objective: "   " }),
    /objective is required/,
  );

  const queuedOnly = latestGoalState([
    {
      type: "custom",
      customType: "goal-state",
      data: { goal: null, queue: [{ id: "next", text: "Next", status: "queued" }] },
    },
  ]);
  assert.equal(queuedOnly.active, undefined);
  assert.equal(queuedOnly.queue[0].id, "next");
});

test("live subagent transcript keeps conversation roles, failures, and a bounded tail", () => {
  const messages = Array.from({ length: MAX_TRANSCRIPT_ENTRIES + 5 }, (_, index) => ({
    role: index % 3 === 0 ? "user" : index % 3 === 1 ? "assistant" : "toolResult",
    content: [{ type: "text", text: `message ${index}` }],
    toolName: index % 3 === 2 ? "read" : undefined,
    isError: index === MAX_TRANSCRIPT_ENTRIES + 4,
  }));
  const transcript = transcriptFromMessages(messages);

  assert.equal(transcript.entries.length, MAX_TRANSCRIPT_ENTRIES);
  assert.equal(transcript.truncated, true);
  assert.equal(transcript.entries[0].content, "message 5");
  assert.equal(transcript.entries.at(-1).isError, true);
  assert.equal(transcript.entries.at(-1).role, "user");
});

test("oversized snapshots shed old transcript payloads instead of breaking the connection", () => {
  const snapshot = {
    sessionId: "session",
    producerId: "adapter-instance",
    generation: 1,
    capturedAt: 1,
    tasks: [],
    subagents: Array.from({ length: 12 }, (_, index) => ({
      id: `agent-${index}`,
      type: "worker",
      description: "Worker",
      status: index === 0 ? "running" : "completed",
      transcript: [
        {
          role: "assistant",
          content: "x".repeat(32 * 1024),
          isError: false,
        },
      ],
      transcriptTruncated: false,
    })),
    schedules: [],
    diagnostics: [],
  };

  const fitted = fitSnapshotRecord(snapshot, 64 * 1024);
  assert.ok(Buffer.byteLength(fitted.encoded, "utf8") <= 64 * 1024);
  assert.equal(fitted.snapshot.producerId, "adapter-instance");
  assert.equal(fitted.snapshot.subagents[0].id, "agent-0");
  assert.ok(
    fitted.snapshot.subagents.some(
      (agent) => agent.transcript.length === 0 && agent.transcriptTruncated,
    ),
  );
});
