#!/usr/bin/env node

import { createInterface } from "node:readline";
import { basename, dirname, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { existsSync, mkdirSync, unlinkSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";

const PROTOCOL_VERSION = 1;
const MAX_LINE_BYTES = 1024 * 1024;
const SDK_VERSION = "0.80.10";
const SESSION_VERSION = 3;
const ORCHESTRATION_ENDPOINT = process.env.PI_GUI_ORCHESTRATION_PIPE;
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
  resourceInventory: true,
  resourceReload: true,
  activeToolState: true,
  resourceSettings: true,
  packageMutations: false,
  orchestration: Boolean(ORCHESTRATION_ENDPOINT),
});

const CAPABILITY_BY_COMMAND = Object.freeze({
  navigate_tree: "navigateTree",
  set_label: "labels",
  export_jsonl: "jsonlExport",
  import_jsonl: "jsonlImport",
  get_model_runtime: "modelRuntime",
  refresh_models: "modelRuntime",
  login_provider: "providerAuth",
  auth_respond: "providerAuth",
  logout_provider: "providerAuth",
  set_model_defaults: "modelSettings",
  set_model_scope: "modelSettings",
  get_resource_inventory: "resourceInventory",
  reload_resources: "resourceReload",
  set_skill_commands_enabled: "resourceSettings",
  set_resource_theme: "resourceSettings",
  get_orchestration_snapshot: "orchestration",
  orchestration_action: "orchestration",
  package_install: "packageMutations",
  package_remove: "packageMutations",
  package_update: "packageMutations",
  package_configure: "packageMutations",
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
let resourceGeneration = 0;
let resourcePlane;
let resourceBuildPromise;
let orchestrationSocket;
let orchestrationSnapshot;
let orchestrationRequestId = 0;
const orchestrationPending = new Map();

function emit(event, value) {
  process.stdout.write(`${JSON.stringify({ version: PROTOCOL_VERSION, type: "event", event, ...value })}\n`);
}

function orchestrationError(message = "The orchestration adapter is unavailable.") {
  const error = new Error(message);
  error.bridgeCode = "orchestration_unavailable";
  return error;
}

function failOrchestrationPending(message) {
  for (const pending of orchestrationPending.values()) {
    clearTimeout(pending.timer);
    pending.reject(orchestrationError(message));
  }
  orchestrationPending.clear();
}

function acceptOrchestrationSocket(socket) {
  orchestrationSocket?.destroy();
  orchestrationSocket = socket;
  socket.setEncoding("utf8");
  const lines = createInterface({ input: socket, crlfDelay: Infinity });
  lines.on("line", (line) => {
    if (Buffer.byteLength(line, "utf8") > MAX_LINE_BYTES) return;
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      return;
    }
    if (record?.type === "snapshot" && record.snapshot && typeof record.snapshot === "object") {
      orchestrationSnapshot = record.snapshot;
      emit("orchestration_snapshot", { snapshot: record.snapshot });
      return;
    }
    if (record?.type !== "response" || typeof record.id !== "string") return;
    const pending = orchestrationPending.get(record.id);
    if (!pending) return;
    orchestrationPending.delete(record.id);
    clearTimeout(pending.timer);
    if (record.ok) pending.resolve(record.result ?? {});
    else pending.reject(orchestrationError("The Pi orchestration action was rejected."));
  });
  socket.on("close", () => {
    if (orchestrationSocket === socket) orchestrationSocket = undefined;
    failOrchestrationPending("The orchestration adapter disconnected.");
    emit("orchestration_disconnected", {});
  });
  socket.on("error", () => {});
}

let orchestrationServer;
if (ORCHESTRATION_ENDPOINT) {
  if (process.platform !== "win32" && existsSync(ORCHESTRATION_ENDPOINT)) {
    try {
      unlinkSync(ORCHESTRATION_ENDPOINT);
    } catch {
      // A live owner will make listen fail; Rust then exposes reconnect recovery.
    }
  }
  orchestrationServer = createServer(acceptOrchestrationSocket);
  orchestrationServer.on("error", () => {
    emit("orchestration_disconnected", {});
  });
  orchestrationServer.listen(ORCHESTRATION_ENDPOINT);
}

function requestOrchestration(action, sessionId) {
  if (!orchestrationSocket || orchestrationSocket.destroyed) {
    return Promise.reject(orchestrationError());
  }
  const id = `orchestration-${++orchestrationRequestId}`;
  return new Promise((resolveRequest, rejectRequest) => {
    const timer = setTimeout(() => {
      orchestrationPending.delete(id);
      rejectRequest(orchestrationError("The orchestration adapter did not answer in time."));
    }, 30_000);
    orchestrationPending.set(id, { resolve: resolveRequest, reject: rejectRequest, timer });
    orchestrationSocket.write(`${JSON.stringify({ type: "request", id, sessionId, action })}\n`, (error) => {
      if (!error) return;
      const pending = orchestrationPending.get(id);
      if (!pending) return;
      orchestrationPending.delete(id);
      clearTimeout(pending.timer);
      rejectRequest(orchestrationError("The orchestration adapter could not receive the action."));
    });
  });
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

function normalizedPath(value) {
  if (typeof value !== "string" || !value) return undefined;
  const normalized = resolve(value);
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function pathContains(parent, child) {
  const normalizedParent = normalizedPath(parent);
  const normalizedChild = normalizedPath(child);
  if (!normalizedParent || !normalizedChild) return false;
  return normalizedChild === normalizedParent || normalizedChild.startsWith(`${normalizedParent}${sep}`);
}

function safePath(value) {
  return typeof value === "string" && value.length <= 4096 ? value : undefined;
}

function safeText(value, fallback, max = 240) {
  if (typeof value !== "string") return fallback;
  const normalized = value.replace(/[\u0000-\u001f\u007f]/g, " ").trim();
  return normalized ? normalized.slice(0, max) : fallback;
}

function resourceScope(sourceInfo = {}) {
  if (sourceInfo.origin === "package") return "package";
  if (sourceInfo.scope === "project") return "project";
  if (sourceInfo.scope === "temporary") return "temporary";
  return "global";
}

function resourceTrust(sourceInfo = {}) {
  return sourceInfo.scope === "project" ? "rejected" : "trusted";
}

function sourceInfoForResolved(resource) {
  return {
    path: resource.path,
    source: resource.metadata?.source ?? "unknown",
    scope: resource.metadata?.scope ?? "user",
    origin: resource.metadata?.origin ?? "top-level",
    baseDir: resource.metadata?.baseDir,
  };
}

function resourceId(kind, path, source, name) {
  return `${kind}:${normalizedPath(path) ?? source ?? name}`;
}

function itemFromSource(kind, name, sourceInfo, values = {}) {
  const path = safePath(values.path ?? sourceInfo?.path);
  return {
    id: resourceId(kind, path, sourceInfo?.source, name),
    kind,
    name: safeText(name, kind),
    description:
      typeof values.description === "string"
        ? safeText(values.description, undefined, 400)
        : undefined,
    state: values.state ?? "loaded",
    scope: resourceScope(sourceInfo),
    ownerScope: sourceInfo?.scope,
    trust: values.trust ?? resourceTrust(sourceInfo),
    path,
    source: safeText(sourceInfo?.source, "unknown"),
    origin: sourceInfo?.origin,
    active: typeof values.active === "boolean" ? values.active : undefined,
    pinned: typeof values.pinned === "boolean" ? values.pinned : undefined,
    filtered: typeof values.filtered === "boolean" ? values.filtered : undefined,
    diagnostics: Array.isArray(values.diagnostics) ? values.diagnostics : [],
  };
}

function safeResourceDiagnostic(diagnostic) {
  if (diagnostic?.type === "collision" && diagnostic.collision) {
    return `${safeText(diagnostic.collision.resourceType, "Resource")} collision for ${safeText(
      diagnostic.collision.name,
      "an unnamed item",
    )}.`;
  }
  return diagnostic?.type === "warning"
    ? "Resource validation reported a warning."
    : "Resource validation reported an error.";
}

function isPinnedPackage(source) {
  if (typeof source !== "string") return false;
  if (/^git\+|^https?:|^ssh:|^[^/]+@[^:]+:/.test(source)) return /#[^#]+$/.test(source);
  const npmVersion = source.startsWith("@")
    ? source.indexOf("@", 1) >= 0
      ? source.slice(source.indexOf("@", 1) + 1)
      : ""
    : source.includes("@")
      ? source.slice(source.lastIndexOf("@") + 1)
      : "";
  return /^\d+\.\d+\.\d+(?:[-+].+)?$/.test(npmVersion);
}

function resourceEntries(resolved) {
  return [
    ["extension", resolved.extensions ?? []],
    ["skill", resolved.skills ?? []],
    ["prompt", resolved.prompts ?? []],
    ["theme", resolved.themes ?? []],
  ];
}

function findResolvedSource(resolved, kind, path) {
  const entries = resourceEntries(resolved).find(([candidate]) => candidate === kind)?.[1] ?? [];
  return entries
    .map((entry) => sourceInfoForResolved(entry))
    .filter(
      (sourceInfo) =>
        pathContains(sourceInfo.path, path) ||
        (sourceInfo.baseDir && pathContains(sourceInfo.baseDir, path)),
    )
    .sort((left, right) => {
      const leftExact = normalizedPath(left.path) === normalizedPath(path) ? 0 : 1;
      const rightExact = normalizedPath(right.path) === normalizedPath(path) ? 0 : 1;
      if (leftExact !== rightExact) return leftExact - rightExact;
      const leftPackage = left.origin === "package" ? 0 : 1;
      const rightPackage = right.origin === "package" ? 0 : 1;
      return leftPackage - rightPackage;
    })[0];
}

function upsertResource(items, item) {
  const existing = items.findIndex(
    (candidate) =>
      candidate.kind === item.kind &&
      ((candidate.path && item.path && pathContains(candidate.path, item.path)) ||
        (candidate.path && item.path && pathContains(item.path, candidate.path)) ||
        candidate.id === item.id),
  );
  if (existing < 0) {
    items.push(item);
    return;
  }
  items[existing] = {
    ...items[existing],
    ...item,
    diagnostics: [...new Set([...(items[existing].diagnostics ?? []), ...(item.diagnostics ?? [])])],
  };
}

async function safeResolve(packageManager) {
  return packageManager.resolve(async () => "skip");
}

function emptyResolvedPaths() {
  return { extensions: [], skills: [], prompts: [], themes: [] };
}

async function buildResourceSnapshot(signal, operation = "inventory") {
  emit("resource_progress", {
    operation,
    phase: "start",
    message: operation === "reload" ? "Reloading installed resources." : "Inspecting installed resources.",
  });
  resourcePlane?.session?.dispose?.();
  resourcePlane = undefined;

  const cwd = process.cwd();
  const agentDir = sdk.getAgentDir();
  const diagnostics = [];
  const globalSettings = sdk.SettingsManager.create(cwd, agentDir, { projectTrusted: false });
  const inspectionSettings = sdk.SettingsManager.create(cwd, agentDir, { projectTrusted: true });
  const globalPackageManager = new sdk.DefaultPackageManager({
    cwd,
    agentDir,
    settingsManager: globalSettings,
  });
  const inspectionPackageManager = new sdk.DefaultPackageManager({
    cwd,
    agentDir,
    settingsManager: inspectionSettings,
  });

  let globalResolved = emptyResolvedPaths();
  let allResolved = emptyResolvedPaths();
  try {
    globalResolved = await safeResolve(globalPackageManager);
  } catch {
    diagnostics.push("Global resource discovery failed; prior files were not changed.");
  }
  try {
    allResolved = await safeResolve(inspectionPackageManager);
  } catch {
    diagnostics.push("Project resource discovery failed; project code was not loaded.");
  }
  if (signal.aborted) return { cancelled: true };

  const additional = Object.fromEntries(
    resourceEntries(globalResolved).map(([kind, resources]) => [
      kind,
      resources.filter((resource) => resource.enabled).map((resource) => resource.path),
    ]),
  );
  const loaderSettings = sdk.SettingsManager.inMemory({}, { projectTrusted: false });
  const loader = new sdk.DefaultResourceLoader({
    cwd,
    agentDir,
    settingsManager: loaderSettings,
    additionalExtensionPaths: additional.extension,
    additionalSkillPaths: additional.skill,
    additionalPromptTemplatePaths: additional.prompt,
    additionalThemePaths: additional.theme,
    noContextFiles: true,
  });

  try {
    await loader.reload();
  } catch {
    diagnostics.push("Installed resource loading failed. Project resources remained disabled.");
  }
  if (signal.aborted) return { cancelled: true };

  const items = [];
  for (const [kind, resources] of resourceEntries(allResolved)) {
    for (const resource of resources) {
      const sourceInfo = sourceInfoForResolved(resource);
      const projectRejected = sourceInfo.scope === "project";
      upsertResource(
        items,
        itemFromSource(kind, basename(resource.path), sourceInfo, {
          path: resource.path,
          state: projectRejected || !resource.enabled ? "disabled" : "loaded",
          diagnostics: projectRejected
            ? ["Project trust is rejected; this resource was inventoried but not loaded."]
            : [],
        }),
      );
    }
  }

  const extensionsResult = loader.getExtensions();
  for (const extension of extensionsResult.extensions ?? []) {
    const sourceInfo =
      findResolvedSource(allResolved, "extension", extension.resolvedPath ?? extension.path) ??
      extension.sourceInfo;
    upsertResource(
      items,
      itemFromSource("extension", basename(extension.path), sourceInfo, {
        path: extension.resolvedPath ?? extension.path,
      }),
    );
  }
  for (const failure of extensionsResult.errors ?? []) {
    const sourceInfo =
      findResolvedSource(allResolved, "extension", failure.path) ?? {
        path: failure.path,
        source: "extension",
        scope: "user",
        origin: "top-level",
      };
    upsertResource(
      items,
      itemFromSource("extension", basename(failure.path), sourceInfo, {
        path: failure.path,
        state: "error",
        diagnostics: ["Extension failed to load. Error details were redacted."],
      }),
    );
  }

  const skills = loader.getSkills();
  for (const skill of skills.skills ?? []) {
    const sourceInfo =
      findResolvedSource(allResolved, "skill", skill.filePath) ?? skill.sourceInfo;
    upsertResource(
      items,
      itemFromSource("skill", skill.name, sourceInfo, {
        path: skill.filePath,
        description: skill.description,
      }),
    );
  }
  const prompts = loader.getPrompts();
  for (const prompt of prompts.prompts ?? []) {
    const sourceInfo =
      findResolvedSource(allResolved, "prompt", prompt.filePath) ?? prompt.sourceInfo;
    upsertResource(
      items,
      itemFromSource("prompt", prompt.name, sourceInfo, {
        path: prompt.filePath,
        description: prompt.description,
      }),
    );
  }
  const themes = loader.getThemes();
  for (const theme of themes.themes ?? []) {
    const sourceInfo =
      findResolvedSource(allResolved, "theme", theme.sourcePath) ?? theme.sourceInfo;
    upsertResource(
      items,
      itemFromSource("theme", theme.name ?? basename(theme.sourcePath ?? "theme"), sourceInfo, {
        path: theme.sourcePath,
      }),
    );
  }
  for (const [kind, resourceDiagnostics] of [
    ["skill", skills.diagnostics ?? []],
    ["prompt", prompts.diagnostics ?? []],
    ["theme", themes.diagnostics ?? []],
  ]) {
    for (const diagnostic of resourceDiagnostics) {
      const sourceInfo =
        findResolvedSource(allResolved, kind, diagnostic.path) ?? {
          path: diagnostic.path,
          source: kind,
          scope: "user",
          origin: "top-level",
        };
      upsertResource(
        items,
        itemFromSource(kind, basename(diagnostic.path ?? kind), sourceInfo, {
          path: diagnostic.path,
          state: diagnostic.type === "warning" ? "loaded" : "error",
          diagnostics: [safeResourceDiagnostic(diagnostic)],
        }),
      );
    }
  }

  const dynamicProviders = [
    ...(extensionsResult.runtime?.pendingProviderRegistrations ?? []),
  ];
  let session;
  try {
    const resourceRuntime = await sdk.ModelRuntime.create({ allowModelNetwork: false });
    const created = await sdk.createAgentSession({
      cwd,
      agentDir,
      modelRuntime: resourceRuntime,
      resourceLoader: loader,
      sessionManager: sdk.SessionManager.inMemory(cwd),
      settingsManager: globalSettings,
    });
    session = created.session;
    const activeTools = new Set(session.getActiveToolNames());
    for (const tool of session.getAllTools()) {
      const sourceInfo =
        findResolvedSource(allResolved, "extension", tool.sourceInfo?.path) ?? tool.sourceInfo;
      upsertResource(
        items,
        itemFromSource("tool", tool.name, sourceInfo, {
          path: tool.sourceInfo?.path,
          description: tool.description,
          active: activeTools.has(tool.name),
        }),
      );
    }
  } catch {
    diagnostics.push("Tool state could not be initialized; resource files remain inventoried.");
  }

  for (const provider of dynamicProviders) {
    const sourceInfo =
      findResolvedSource(allResolved, "extension", provider.extensionPath) ?? {
        path: provider.extensionPath,
        source: "extension",
        scope: "user",
        origin: "top-level",
      };
    upsertResource(
      items,
      itemFromSource("provider", provider.name, sourceInfo, {
        path: provider.extensionPath,
        description: "Provider registered dynamically by an extension.",
      }),
    );
  }

  let configuredPackages = [];
  try {
    configuredPackages = inspectionPackageManager.listConfiguredPackages();
  } catch {
    diagnostics.push("Package configuration could not be inventoried.");
  }
  for (const pkg of configuredPackages) {
    const sourceInfo = {
      path: pkg.installedPath,
      source: pkg.source,
      scope: pkg.scope,
      origin: "package",
      baseDir: pkg.installedPath,
    };
    const projectRejected = pkg.scope === "project";
    const installed = typeof pkg.installedPath === "string";
    items.push(
      itemFromSource("package", pkg.source, sourceInfo, {
        path: pkg.installedPath,
        state: !installed ? "error" : projectRejected ? "disabled" : "loaded",
        trust: projectRejected ? "rejected" : "trusted",
        pinned: isPinnedPackage(pkg.source),
        filtered: pkg.filtered === true,
        diagnostics: !installed
          ? ["Package is configured but not installed. Automatic installation is disabled."]
          : projectRejected
            ? ["Project trust is rejected; package code was not loaded."]
            : [],
      }),
    );
  }

  try {
    for (const context of sdk.loadProjectContextFiles({ cwd, agentDir })) {
      const global = pathContains(agentDir, context.path);
      const sourceInfo = {
        path: context.path,
        source: global ? "global-context" : "project-context",
        scope: global ? "user" : "project",
        origin: "top-level",
      };
      items.push(
        itemFromSource("context", basename(context.path), sourceInfo, {
          path: context.path,
          state: "disabled",
          trust: global ? "trusted" : "rejected",
          diagnostics: [
            global
              ? "Context is inventoried but disabled because the native shell does not consume it."
              : "Project trust is rejected; context was inventoried but not loaded.",
          ],
        }),
      );
    }
  } catch {
    diagnostics.push("Context-file discovery failed; no file contents were exposed.");
  }

  diagnostics.push(
    ...globalSettings.drainErrors().map(() => "A global Pi settings read reported an error."),
    ...inspectionSettings
      .drainErrors()
      .map(() => "A project Pi settings read reported an error; details were redacted."),
  );
  items.sort(
    (left, right) =>
      left.kind.localeCompare(right.kind) ||
      left.scope.localeCompare(right.scope) ||
      left.name.localeCompare(right.name),
  );
  resourceGeneration += 1;
  const snapshot = {
    generation: resourceGeneration,
    projectTrusted: false,
    projectTrustReason:
      "Pi GUI rejects project trust. Project resources are inventoried without executing project code.",
    items,
    diagnostics,
    settings: {
      enableSkillCommands: globalSettings.getEnableSkillCommands(),
      theme: globalSettings.getThemeSetting(),
      defaultProjectTrust: globalSettings.getDefaultProjectTrust(),
    },
    packageMutations: {
      install: false,
      remove: false,
      update: false,
      configure: false,
      reason:
        "Disabled until arbitrary-code confirmation, progress, pin/filter handling, and rollback-safe errors are implemented.",
    },
  };
  resourcePlane = { session, loader, snapshot };
  emit("resource_progress", {
    operation,
    phase: "complete",
    message: "Installed resource inventory is ready.",
  });
  emit("resources_changed", { generation: snapshot.generation });
  return snapshot;
}

async function resourceSnapshot(signal, reload = false) {
  if (!reload && resourcePlane?.snapshot) return resourcePlane.snapshot;
  if (!resourceBuildPromise) {
    resourceBuildPromise = buildResourceSnapshot(signal, reload ? "reload" : "inventory").finally(
      () => {
        resourceBuildPromise = undefined;
      },
    );
  }
  return resourceBuildPromise;
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
          transport: "stdio-jsonl",
          ownership: "pi-sdk-sidecar",
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
      case "get_resource_inventory":
        result = await resourceSnapshot(token.abortController.signal);
        break;
      case "reload_resources":
        result = await resourceSnapshot(token.abortController.signal, true);
        break;
      case "set_skill_commands_enabled": {
        if (typeof params.enabled !== "boolean") throw new Error("Invalid enabled value");
        const { settings } = await getModelServices();
        settings.setEnableSkillCommands(params.enabled);
        await settings.flush();
        resourcePlane?.session?.dispose?.();
        resourcePlane = undefined;
        result = await resourceSnapshot(token.abortController.signal, true);
        break;
      }
      case "set_resource_theme": {
        const theme = requireString(params, "theme");
        const current = await resourceSnapshot(token.abortController.signal);
        const available = current.items?.some(
          (item) => item.kind === "theme" && item.state === "loaded" && item.name === theme,
        );
        if (!available) throw new Error("Theme is not available");
        const { settings } = await getModelServices();
        settings.setTheme(theme);
        await settings.flush();
        resourcePlane?.session?.dispose?.();
        resourcePlane = undefined;
        result = await resourceSnapshot(token.abortController.signal, true);
        break;
      }
      case "get_orchestration_snapshot": {
        const requestedSession =
          typeof params.sessionId === "string" && params.sessionId ? params.sessionId : undefined;
        if (
          orchestrationSnapshot &&
          (!requestedSession || orchestrationSnapshot.sessionId === requestedSession)
        ) {
          result = orchestrationSnapshot;
          break;
        }
        result = await requestOrchestration({ kind: "snapshot" }, requestedSession);
        break;
      }
      case "orchestration_action":
        if (!params.action || typeof params.action !== "object") {
          throw orchestrationError("The orchestration action was invalid.");
        }
        result = await requestOrchestration(params.action, params.action.sessionId);
        break;
      default: {
        const unsupported = new Error("Unsupported bridge command");
        unsupported.bridgeCode = "unsupported_command";
        throw unsupported;
      }
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
      "get_resource_inventory",
      "reload_resources",
      "set_skill_commands_enabled",
      "set_resource_theme",
      "get_orchestration_snapshot",
      "orchestration_action",
    ].includes(command);
    const message = sensitiveOperation
      ? command.includes("orchestration")
        ? "The orchestration operation failed."
        : command.includes("resource") || command.includes("skill")
        ? "The resource operation failed. Details were redacted."
        : "The model or authentication operation failed."
      : error instanceof Error
        ? "The bridge operation failed."
        : "Bridge operation failed";
    respond(id, false, {
      code: error?.bridgeCode ?? "operation_failed",
      message: message.slice(0, 400),
    });
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
  const capability = CAPABILITY_BY_COMMAND[record.command];
  if (capability && CAPABILITIES[capability] !== true) {
    respond(record.id, false, {
      code: "unsupported_capability",
      message: "The negotiated bridge does not support this operation.",
    });
    return;
  }
  void execute(record);
});

input.on("close", () => {
  for (const operation of active.values()) {
    operation.abortController.abort();
    operation.session?.abortBranchSummary();
  }
  resourcePlane?.session?.dispose?.();
  failOrchestrationPending("The Pi SDK bridge stopped.");
  orchestrationSocket?.destroy();
  orchestrationServer?.close();
  if (ORCHESTRATION_ENDPOINT && process.platform !== "win32") {
    try {
      unlinkSync(ORCHESTRATION_ENDPOINT);
    } catch {
      // The endpoint may already have been removed by the server shutdown.
    }
  }
});
