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
  modelRuntime: true,
  providerAuth: true,
  modelSettings: true,
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
const authPrompts = new Map();
let modelRuntimePromise;
let modelRefreshPromise;
let modelRefreshErrors = new Map();

function emit(event, value) {
  process.stdout.write(`${JSON.stringify({ version: PROTOCOL_VERSION, type: "event", event, ...value })}\n`);
}

function safeAuthSource(source) {
  switch (source) {
    case "stored":
    case "runtime":
    case "environment":
    case "fallback":
      return source;
    case "models_json_key":
    case "models_json_command":
      return "models_json";
    default:
      return source ? "unknown" : undefined;
  }
}

function supportedThinking(model) {
  const levels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
  if (!model.reasoning) return ["off"];
  return levels.filter((level) => {
    const mapped = model.thinkingLevelMap?.[level];
    if (mapped === null) return false;
    if (level === "xhigh" || level === "max") return mapped !== undefined;
    return true;
  });
}

async function getModelServices() {
  if (!modelRuntimePromise) {
    modelRuntimePromise = (async () => {
      const runtime = await sdk.ModelRuntime.create({ allowModelNetwork: false });
      const settings = sdk.SettingsManager.create(process.cwd(), sdk.getAgentDir(), {
        projectTrusted: false,
      });
      return { runtime, settings };
    })();
  }
  return modelRuntimePromise;
}

async function modelSnapshot() {
  const { runtime, settings } = await getModelServices();
  const allModels = [...runtime.getModels()];
  const available = new Set(
    runtime.getAvailableSnapshot().map((model) => `${model.provider}\0${model.id}`),
  );
  const providers = runtime.getProviders().map((provider) => {
    const models = allModels.filter((model) => model.provider === provider.id);
    const auth = runtime.getProviderAuthStatus(provider.id);
    return {
      id: provider.id,
      name: provider.name || provider.id,
      authMethods: [
        ...(typeof provider.auth?.apiKey?.login === "function" ? ["api_key"] : []),
        ...(typeof provider.auth?.oauth?.login === "function" ? ["oauth"] : []),
      ],
      auth: {
        configured: auth.configured === true,
        source: safeAuthSource(auth.source),
      },
      modelCount: models.length,
      availableModelCount: models.filter((model) => available.has(`${model.provider}\0${model.id}`)).length,
      refreshError: modelRefreshErrors.has(provider.id) ? "Refresh failed; cached models retained." : undefined,
    };
  });
  const defaultProvider = settings.getDefaultProvider();
  const defaultModel = settings.getDefaultModel();
  const enabledModels = settings.getEnabledModels() ?? [];
  const scopedModels = enabledModels.flatMap((pattern) => {
    const separator = pattern.indexOf("/");
    if (separator <= 0) return [];
    const provider = pattern.slice(0, separator);
    const id = pattern.slice(separator + 1);
    return runtime.getModel(provider, id) ? [{ provider, id }] : [];
  });
  return {
    providers,
    models: allModels.map((model) => ({
      provider: model.provider,
      id: model.id,
      name: model.name,
      api: model.api,
      reasoning: model.reasoning,
      supportsImages: model.input.includes("image"),
      contextWindow: model.contextWindow,
      maxTokens: model.maxTokens,
      pricing: {
        input: model.cost.input,
        output: model.cost.output,
        cacheRead: model.cost.cacheRead,
        cacheWrite: model.cost.cacheWrite,
        tiers: (model.cost.tiers ?? []).map((tier) => ({
          inputTokensAbove: tier.inputTokensAbove,
          input: tier.input,
          output: tier.output,
          cacheRead: tier.cacheRead,
          cacheWrite: tier.cacheWrite,
        })),
      },
      supportedThinking: supportedThinking(model),
      thinkingLevelMap: model.thinkingLevelMap ?? {},
      available: available.has(`${model.provider}\0${model.id}`),
    })),
    defaults: {
      model: defaultProvider && defaultModel ? { provider: defaultProvider, id: defaultModel } : undefined,
      thinking: settings.getDefaultThinkingLevel(),
      scopedModels,
    },
    diagnostics: [
      ...(runtime.getError() ? ["Model configuration has diagnostics. Details are kept out of the GUI."] : []),
      ...settings.drainErrors().map(() => "A Pi settings operation reported an error."),
    ],
  };
}

async function refreshModels(signal) {
  if (!modelRefreshPromise) {
    modelRefreshPromise = (async () => {
      const { runtime } = await getModelServices();
      const result = await runtime.refresh({ allowNetwork: true, force: true, signal });
      modelRefreshErrors = new Map(result.errors);
      return modelSnapshot();
    })().finally(() => {
      modelRefreshPromise = undefined;
    });
  }
  return modelRefreshPromise;
}

function authInteraction(operationId, signal) {
  let nextPrompt = 1;
  return {
    signal,
    prompt(prompt) {
      const promptId = `prompt-${nextPrompt++}`;
      return new Promise((resolvePrompt, rejectPrompt) => {
        const key = `${operationId}:${promptId}`;
        const abort = () => {
          authPrompts.delete(key);
          rejectPrompt(new Error("Authentication cancelled"));
        };
        if (signal.aborted || prompt.signal?.aborted) return abort();
        signal.addEventListener("abort", abort, { once: true });
        prompt.signal?.addEventListener("abort", abort, { once: true });
        authPrompts.set(key, {
          resolve(value) {
            signal.removeEventListener("abort", abort);
            prompt.signal?.removeEventListener("abort", abort);
            resolvePrompt(value);
          },
        });
        emit("auth_prompt", {
          operationId,
          prompt: {
            promptId,
            kind: prompt.type,
            message: prompt.message,
            placeholder: prompt.placeholder,
            options: prompt.options ?? [],
          },
        });
      });
    },
    notify(event) {
      switch (event.type) {
        case "info":
          emit("auth_info", { operationId, message: event.message, links: event.links ?? [] });
          break;
        case "auth_url":
          emit("auth_url", { operationId, url: event.url, instructions: event.instructions });
          break;
        case "device_code":
          emit("auth_device_code", {
            operationId,
            userCode: event.userCode,
            verificationUri: event.verificationUri,
            expiresInSeconds: event.expiresInSeconds,
          });
          break;
        case "progress":
          emit("auth_progress", { operationId, message: event.message });
          break;
      }
    },
  };
}

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
  const token = { cancelled: false, session: undefined, abortController: new AbortController() };
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
      case "get_model_runtime":
        result = await modelSnapshot();
        break;
      case "refresh_models":
        result = await refreshModels(token.abortController.signal);
        break;
      case "login_provider": {
        const provider = requireString(params, "provider");
        const operationId = params.operationId;
        if (!Number.isSafeInteger(operationId)) throw new Error("Missing operationId");
        const authType = params.authType;
        if (authType !== "api_key" && authType !== "oauth") throw new Error("Invalid authType");
        const { runtime } = await getModelServices();
        await runtime.login(
          provider,
          authType,
          authInteraction(operationId, token.abortController.signal),
        );
        result = await modelSnapshot();
        break;
      }
      case "auth_respond": {
        const operationId = params.operationId;
        const promptId = requireString(params, "promptId");
        if (!Number.isSafeInteger(operationId)) throw new Error("Missing operationId");
        if (typeof params.value !== "string") throw new Error("Missing prompt value");
        const key = `${operationId}:${promptId}`;
        const prompt = authPrompts.get(key);
        if (!prompt) throw new Error("Authentication prompt is no longer active");
        authPrompts.delete(key);
        prompt.resolve(params.value);
        result = { accepted: true };
        break;
      }
      case "logout_provider": {
        const provider = requireString(params, "provider");
        const { runtime } = await getModelServices();
        await runtime.logout(provider);
        const snapshot = await modelSnapshot();
        const status = snapshot.providers.find((item) => item.id === provider)?.auth;
        result = { snapshot, environmentFallback: status?.configured === true && status.source === "environment" };
        break;
      }
      case "set_model_defaults": {
        const { settings } = await getModelServices();
        if (params.model) {
          settings.setDefaultModelAndProvider(
            requireString(params.model, "provider"),
            requireString(params.model, "id"),
          );
        }
        if (typeof params.thinking === "string") settings.setDefaultThinkingLevel(params.thinking);
        await settings.flush();
        result = await modelSnapshot();
        break;
      }
      case "set_model_scope": {
        const models = Array.isArray(params.models) ? params.models : [];
        const patterns = models.map((model) => `${requireString(model, "provider")}/${requireString(model, "id")}`);
        const { settings } = await getModelServices();
        settings.setEnabledModels(patterns.length > 0 ? patterns : undefined);
        await settings.flush();
        result = await modelSnapshot();
        break;
      }
      default:
        throw new Error("Unsupported bridge command");
    }
    respond(id, true, result);
  } catch (error) {
    const sensitiveOperation = [
      "get_model_runtime",
      "refresh_models",
      "login_provider",
      "auth_respond",
      "logout_provider",
      "set_model_defaults",
      "set_model_scope",
    ].includes(command);
    const message = sensitiveOperation
      ? "The model or authentication operation failed."
      : error instanceof Error
        ? error.message
        : "Bridge operation failed";
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
      operation.abortController.abort();
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
