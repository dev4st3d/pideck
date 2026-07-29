import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
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

const FAKE_SDK = String.raw`
const values = {
  steeringMode: "one-at-a-time",
  followUpMode: "one-at-a-time",
  transport: "auto",
  compactionEnabled: true,
  retryEnabled: true,
  hideThinkingBlock: false,
  showCacheMissNotices: false,
  quietStartup: false,
  defaultProjectTrust: "ask",
  collapseChangelog: false,
  enableInstallTelemetry: true,
  enableAnalytics: false,
  showImages: true,
  clearOnShrink: false,
  imageAutoResize: true,
  blockImages: false,
  doubleEscapeAction: "tree",
  treeFilterMode: "default",
  showHardwareCursor: false,
  editorPaddingX: 0,
  outputPad: 1,
  autocompleteMaxVisible: 5,
  enableSkillCommands: true,
};

function settings() {
  return {
    getDefaultProvider: () => undefined,
    getDefaultModel: () => undefined,
    getEnabledModels: () => undefined,
    getDefaultThinkingLevel: () => undefined,
    drainErrors: () => [],
    flush: async () => {},
    getSteeringMode: () => values.steeringMode,
    setSteeringMode: (value) => { values.steeringMode = value; },
    getFollowUpMode: () => values.followUpMode,
    setFollowUpMode: (value) => { values.followUpMode = value; },
    getTransport: () => values.transport,
    setTransport: (value) => { values.transport = value; },
    getCompactionEnabled: () => values.compactionEnabled,
    setCompactionEnabled: (value) => { values.compactionEnabled = value; },
    getRetryEnabled: () => values.retryEnabled,
    setRetryEnabled: (value) => { values.retryEnabled = value; },
    getHideThinkingBlock: () => values.hideThinkingBlock,
    setHideThinkingBlock: (value) => { values.hideThinkingBlock = value; },
    getShowCacheMissNotices: () => values.showCacheMissNotices,
    setShowCacheMissNotices: (value) => { values.showCacheMissNotices = value; },
    getQuietStartup: () => values.quietStartup,
    setQuietStartup: (value) => { values.quietStartup = value; },
    getDefaultProjectTrust: () => values.defaultProjectTrust,
    setDefaultProjectTrust: (value) => { values.defaultProjectTrust = value; },
    getCollapseChangelog: () => values.collapseChangelog,
    setCollapseChangelog: (value) => { values.collapseChangelog = value; },
    getEnableInstallTelemetry: () => values.enableInstallTelemetry,
    setEnableInstallTelemetry: (value) => { values.enableInstallTelemetry = value; },
    getEnableAnalytics: () => values.enableAnalytics,
    setEnableAnalytics: (value) => { values.enableAnalytics = value; },
    getShowImages: () => values.showImages,
    setShowImages: (value) => { values.showImages = value; },
    getClearOnShrink: () => values.clearOnShrink,
    setClearOnShrink: (value) => { values.clearOnShrink = value; },
    getImageAutoResize: () => values.imageAutoResize,
    setImageAutoResize: (value) => { values.imageAutoResize = value; },
    getBlockImages: () => values.blockImages,
    setBlockImages: (value) => { values.blockImages = value; },
    getDoubleEscapeAction: () => values.doubleEscapeAction,
    setDoubleEscapeAction: (value) => { values.doubleEscapeAction = value; },
    getTreeFilterMode: () => values.treeFilterMode,
    setTreeFilterMode: (value) => { values.treeFilterMode = value; },
    getShowHardwareCursor: () => values.showHardwareCursor,
    setShowHardwareCursor: (value) => { values.showHardwareCursor = value; },
    getEditorPaddingX: () => values.editorPaddingX,
    setEditorPaddingX: (value) => { values.editorPaddingX = value; },
    getOutputPad: () => values.outputPad,
    setOutputPad: (value) => { values.outputPad = value; },
    getAutocompleteMaxVisible: () => values.autocompleteMaxVisible,
    setAutocompleteMaxVisible: (value) => { values.autocompleteMaxVisible = value; },
    getEnableSkillCommands: () => values.enableSkillCommands,
    setEnableSkillCommands: (value) => { values.enableSkillCommands = value; },
  };
}

export const ModelRuntime = {
  async create() {
    return {
      getModels: () => [],
      getAvailableSnapshot: () => [],
      getProviders: () => [],
      getProviderAuthStatus: () => ({ configured: false }),
      getError: () => undefined,
      getModel: () => undefined,
    };
  },
};
export const SettingsManager = { create: settings };
export const getAgentDir = () => ".pi";
`;

test("model bridge snapshots and validates Pi settings end to end", { timeout: 10_000 }, async () => {
  const root = mkdtempSync(join(tmpdir(), "pi-gui-model-settings-"));
  const sdkRoot = join(root, "sdk");
  mkdirSync(join(sdkRoot, "dist"), { recursive: true });
  writeFileSync(join(sdkRoot, "package.json"), '{"version":"0.82.1","type":"module"}\n');
  writeFileSync(join(sdkRoot, "dist", "index.js"), FAKE_SDK);

  const child = spawn(
    process.execPath,
    [fileURLToPath(new URL("./pi-bridge.mjs", import.meta.url)), sdkRoot],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  const records = [];
  let stderr = "";
  attachJsonlLineReader(child.stdout, (line) => records.push(JSON.parse(line)));
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => { stderr += chunk; });

  const request = (id, command, params = {}) => {
    child.stdin.write(serializeJsonLine({ version: 1, type: "request", id, command, params }));
  };

  try {
    request("snapshot", "get_model_runtime");
    const snapshot = await waitFor(
      () => records.find((record) => record.id === "snapshot"),
      "model snapshot was not returned",
    );
    assert.equal(snapshot.ok, true);
    assert.equal(snapshot.result.settings.retryEnabled, true);
    assert.equal(snapshot.result.settings.transport, "auto");

    request("set", "set_pi_setting", { key: "retry.enabled", value: false });
    const saved = await waitFor(
      () => records.find((record) => record.id === "set"),
      "Pi setting result was not returned",
    );
    assert.equal(saved.ok, true);
    assert.equal(saved.result.settings.retryEnabled, false);

    request("invalid", "set_pi_setting", { key: "editorPaddingX", value: 99 });
    const rejected = await waitFor(
      () => records.find((record) => record.id === "invalid"),
      "invalid Pi setting was not rejected",
    );
    assert.equal(rejected.ok, false);
    assert.equal(rejected.error.code, "invalid_setting");
    assert.match(rejected.error.message, /model or authentication operation failed/i);
  } finally {
    child.stdin.end();
    const exited = await Promise.race([
      once(child, "exit").then(() => true),
      new Promise((resolve) => setTimeout(() => resolve(false), 2_000)),
    ]);
    if (!exited) child.kill("SIGKILL");
    assert.equal(stderr, "");
    rmSync(root, { recursive: true, force: true });
  }
});
