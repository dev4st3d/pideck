import assert from "node:assert/strict";
import { test } from "node:test";

import { piSettingsSnapshot, setPiSetting } from "./pi-settings.mjs";

function fakeSettings() {
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
  const settings = {};
  for (const [name, value] of Object.entries(values)) {
    settings[`get${name[0].toUpperCase()}${name.slice(1)}`] = () => values[name];
  }
  const setterNames = {
    setSteeringMode: "steeringMode",
    setFollowUpMode: "followUpMode",
    setTransport: "transport",
    setCompactionEnabled: "compactionEnabled",
    setRetryEnabled: "retryEnabled",
    setHideThinkingBlock: "hideThinkingBlock",
    setShowCacheMissNotices: "showCacheMissNotices",
    setQuietStartup: "quietStartup",
    setDefaultProjectTrust: "defaultProjectTrust",
    setCollapseChangelog: "collapseChangelog",
    setEnableInstallTelemetry: "enableInstallTelemetry",
    setEnableAnalytics: "enableAnalytics",
    setShowImages: "showImages",
    setClearOnShrink: "clearOnShrink",
    setImageAutoResize: "imageAutoResize",
    setBlockImages: "blockImages",
    setDoubleEscapeAction: "doubleEscapeAction",
    setTreeFilterMode: "treeFilterMode",
    setShowHardwareCursor: "showHardwareCursor",
    setEditorPaddingX: "editorPaddingX",
    setOutputPad: "outputPad",
    setAutocompleteMaxVisible: "autocompleteMaxVisible",
    setEnableSkillCommands: "enableSkillCommands",
  };
  for (const [setter, name] of Object.entries(setterNames)) {
    settings[setter] = (value) => {
      values[name] = value;
    };
  }
  return { settings, values };
}

test("Pi setting snapshot exposes typed effective values", () => {
  const { settings } = fakeSettings();
  assert.deepEqual(piSettingsSnapshot(settings), {
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
  });
});

test("Pi setting writer validates booleans, enums, and bounded integers", () => {
  const { settings, values } = fakeSettings();
  setPiSetting(settings, "transport", "websocket-cached");
  setPiSetting(settings, "retry.enabled", false);
  setPiSetting(settings, "autocompleteMaxVisible", 12);
  assert.equal(values.transport, "websocket-cached");
  assert.equal(values.retryEnabled, false);
  assert.equal(values.autocompleteMaxVisible, 12);

  assert.throws(() => setPiSetting(settings, "retry.enabled", "false"), /must be a boolean/);
  assert.throws(() => setPiSetting(settings, "transport", "udp"), /must be one of/);
  assert.throws(() => setPiSetting(settings, "editorPaddingX", 4), /integer from 0 to 3/);
  assert.throws(() => setPiSetting(settings, "unknown", true), /Unsupported Pi setting/);
});
