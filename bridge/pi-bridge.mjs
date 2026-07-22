#!/usr/bin/env node

import { createInterface } from "node:readline";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";

const PROTOCOL_VERSION = 1;
const MAX_LINE_BYTES = 1024 * 1024;
const SDK_VERSION = "0.80.10";
const SESSION_VERSION = 3;
const CAPABILITIES = Object.freeze({
  navigateTree: true,
  branchSummary: true,
  labels: true,
  jsonlImport: true,
  jsonlExport: true,
  sessionList: false,
});

const sdkRoot = process.argv[2];
if (!sdkRoot) {
  process.stderr.write("pi-gui bridge requires the Pi SDK package root\n");
  process.exit(2);
}

let sdk;
try {
  sdk = await import(pathToFileURL(resolve(sdkRoot, "dist", "index.js")).href);
} catch {
  process.stderr.write("pi-gui bridge could not load the compatible Pi SDK\n");
  process.exit(2);
}

const active = new Map();

function respond(id, ok, value) {
  const record = ok
    ? { version: PROTOCOL_VERSION, type: "response", id, ok: true, result: value }
    : { version: PROTOCOL_VERSION, type: "response", id, ok: false, error: value };
  process.stdout.write(`${JSON.stringify(record)}\n`);
}

function requireString(params, name) {
  const value = params?.[name];
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new Error(`Missing ${name}`);
  }
  return value;
}

async function withSession(params, operation) {
  const sessionPath = requireString(params, "sessionPath");
  const cwd = requireString(params, "cwd");
  const sessionManager = sdk.SessionManager.open(sessionPath, undefined, cwd);
  const { session } = await sdk.createAgentSession({
    cwd,
    sessionManager,
    noTools: "all",
  });
  try {
    return await operation(session);
  } finally {
    session.dispose();
  }
}

function editorTextForEntry(entry) {
  if (entry.type === "message" && entry.message?.role === "user") {
    const content = entry.message.content;
    return typeof content === "string"
      ? content
      : Array.isArray(content)
        ? content.filter((part) => part?.type === "text").map((part) => part.text ?? "").join("")
        : "";
  }
  if (entry.type === "custom_message") {
    return typeof entry.content === "string"
      ? entry.content
      : Array.isArray(entry.content)
        ? entry.content.filter((part) => part?.type === "text").map((part) => part.text ?? "").join("")
        : "";
  }
  return undefined;
}

function navigateWithoutSummary(params) {
  const sessionManager = sdk.SessionManager.open(
    requireString(params, "sessionPath"),
    undefined,
    requireString(params, "cwd"),
  );
  const targetId = requireString(params, "targetId");
  const targetEntry = sessionManager.getEntry(targetId);
  if (!targetEntry) throw new Error("Target entry was not found");
  if (targetId === sessionManager.getLeafId()) {
    return { cancelled: false, leafId: targetId };
  }
  const editorText = editorTextForEntry(targetEntry);
  const beforeEntry = editorText !== undefined;
  const newLeafId = beforeEntry ? targetEntry.parentId : targetId;
  if (newLeafId === null) sessionManager.resetLeaf();
  else sessionManager.branch(newLeafId);
  const label = typeof params.label === "string" && params.label.trim() ? params.label.trim() : undefined;
  const checkpointId = sessionManager.appendLabelChange(
    targetId,
    label ?? sessionManager.getLabel(targetId),
  );
  return {
    cancelled: false,
    editorText,
    leafId: sessionManager.getLeafId(),
    checkpointId,
  };
}

function exportActivePath(params) {
  const sessionManager = sdk.SessionManager.open(
    requireString(params, "sessionPath"),
    undefined,
    requireString(params, "cwd"),
  );
  const requested =
    typeof params.outputPath === "string" && params.outputPath.trim()
      ? params.outputPath.trim()
      : `session-${new Date().toISOString().replace(/[:.]/g, "-")}.jsonl`;
  const outputPath = resolve(process.cwd(), requested);
  const outputDirectory = dirname(outputPath);
  if (!existsSync(outputDirectory)) mkdirSync(outputDirectory, { recursive: true });
  const header = {
    type: "session",
    version: SESSION_VERSION,
    id: sessionManager.getSessionId(),
    timestamp: new Date().toISOString(),
    cwd: sessionManager.getCwd(),
  };
  let parentId = null;
  const entries = sessionManager.getBranch().map((entry) => {
    const linear = { ...entry, parentId };
    parentId = entry.id;
    return linear;
  });
  writeFileSync(outputPath, `${[header, ...entries].map((entry) => JSON.stringify(entry)).join("\n")}\n`);
  return outputPath;
}

async function execute(record) {
  const { id, command, params = {} } = record;
  const token = { cancelled: false, session: undefined };
  active.set(id, token);
  try {
    let result;
    switch (command) {
      case "hello":
        result = {
          protocolVersion: PROTOCOL_VERSION,
          sdkVersion: SDK_VERSION,
          capabilities: CAPABILITIES,
        };
        break;
      case "navigate_tree":
        if (params.summarize !== true) {
          result = token.cancelled ? { cancelled: true } : navigateWithoutSummary(params);
          break;
        }
        result = await withSession(params, async (session) => {
          token.session = session;
          if (token.cancelled) return { cancelled: true };
          const navigation = await session.navigateTree(requireString(params, "targetId"), {
            summarize: params.summarize === true,
            customInstructions:
              typeof params.customInstructions === "string" ? params.customInstructions : undefined,
            replaceInstructions: params.replaceInstructions === true,
            label: typeof params.label === "string" ? params.label : undefined,
          });
          let checkpointId;
          if (!navigation.cancelled && !navigation.summaryEntry && !params.label) {
            const targetId = requireString(params, "targetId");
            checkpointId = session.sessionManager.appendLabelChange(
              targetId,
              session.sessionManager.getLabel(targetId),
            );
          }
          return {
            cancelled: navigation.cancelled,
            aborted: navigation.aborted === true,
            editorText: navigation.editorText,
            leafId: session.sessionManager.getLeafId(),
            summaryEntryId: navigation.summaryEntry?.id,
            checkpointId,
          };
        });
        break;
      case "set_label": {
        const sessionManager = sdk.SessionManager.open(
          requireString(params, "sessionPath"),
          undefined,
          requireString(params, "cwd"),
        );
        const targetId = requireString(params, "targetId");
        if (!sessionManager.getEntry(targetId)) throw new Error("Target entry was not found");
        if (token.cancelled) {
          result = { cancelled: true };
          break;
        }
        const label = typeof params.label === "string" ? params.label.trim() : "";
        sessionManager.appendLabelChange(targetId, label || undefined);
        result = { cancelled: false, targetId, label: label || undefined };
        break;
      }
      case "export_jsonl":
        result = token.cancelled
          ? { cancelled: true }
          : { cancelled: false, path: exportActivePath(params) };
        break;
      case "import_jsonl": {
        const inputPath = requireString(params, "inputPath");
        const cwd = requireString(params, "cwd");
        const sessionDir = requireString(params, "sessionDir");
        sdk.SessionManager.open(inputPath);
        if (token.cancelled) {
          result = { cancelled: true };
          break;
        }
        const imported = sdk.SessionManager.forkFrom(inputPath, cwd, sessionDir, {
          parentSession: inputPath,
        });
        result = {
          cancelled: false,
          path: imported.getSessionFile(),
          sessionId: imported.getSessionId(),
        };
        break;
      }
      default:
        throw new Error("Unsupported bridge command");
    }
    respond(id, true, result);
  } catch (error) {
    const message = error instanceof Error ? error.message : "Bridge operation failed";
    respond(id, false, { code: "operation_failed", message: message.slice(0, 400) });
  } finally {
    active.delete(id);
  }
}

const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", (line) => {
  if (Buffer.byteLength(line, "utf8") > MAX_LINE_BYTES) {
    respond("oversized", false, { code: "record_too_large", message: "Bridge record is too large" });
    return;
  }
  let record;
  try {
    record = JSON.parse(line);
  } catch {
    respond("malformed", false, { code: "invalid_json", message: "Bridge record is not valid JSON" });
    return;
  }
  if (record?.version !== PROTOCOL_VERSION || typeof record?.id !== "string") {
    respond(record?.id ?? "invalid", false, {
      code: "incompatible_protocol",
      message: "Unsupported bridge protocol version",
    });
    return;
  }
  if (record.type === "cancel") {
    const operation = active.get(record.targetId);
    if (operation) {
      operation.cancelled = true;
      operation.session?.abortBranchSummary();
    }
    respond(record.id, true, { cancelled: Boolean(operation) });
    return;
  }
  if (record.type !== "request" || typeof record.command !== "string") {
    respond(record.id, false, { code: "invalid_request", message: "Invalid bridge request" });
    return;
  }
  void execute(record);
});

input.on("close", () => {
  for (const operation of active.values()) operation.session?.abortBranchSummary();
});
