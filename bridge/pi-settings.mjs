function invalidSetting(message) {
  const error = new Error(message);
  error.bridgeCode = "invalid_setting";
  return error;
}

function requireBoolean(value, key) {
  if (typeof value !== "boolean") {
    throw invalidSetting(`${key} must be a boolean`);
  }
  return value;
}

function requireEnum(value, key, options) {
  if (typeof value !== "string" || !options.includes(value)) {
    throw invalidSetting(`${key} must be one of: ${options.join(", ")}`);
  }
  return value;
}

function requireInteger(value, key, minimum, maximum) {
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    throw invalidSetting(`${key} must be an integer from ${minimum} to ${maximum}`);
  }
  return value;
}

const BOOLEAN_SETTERS = Object.freeze({
  "compaction.enabled": "setCompactionEnabled",
  "retry.enabled": "setRetryEnabled",
  hideThinkingBlock: "setHideThinkingBlock",
  showCacheMissNotices: "setShowCacheMissNotices",
  quietStartup: "setQuietStartup",
  collapseChangelog: "setCollapseChangelog",
  enableInstallTelemetry: "setEnableInstallTelemetry",
  enableAnalytics: "setEnableAnalytics",
  "terminal.showImages": "setShowImages",
  "terminal.clearOnShrink": "setClearOnShrink",
  "images.autoResize": "setImageAutoResize",
  "images.blockImages": "setBlockImages",
  showHardwareCursor: "setShowHardwareCursor",
  enableSkillCommands: "setEnableSkillCommands",
});

const ENUM_SETTERS = Object.freeze({
  steeringMode: ["setSteeringMode", ["all", "one-at-a-time"]],
  followUpMode: ["setFollowUpMode", ["all", "one-at-a-time"]],
  transport: ["setTransport", ["auto", "sse", "websocket", "websocket-cached"]],
  defaultProjectTrust: ["setDefaultProjectTrust", ["ask", "always", "never"]],
  doubleEscapeAction: ["setDoubleEscapeAction", ["tree", "fork", "none"]],
  treeFilterMode: [
    "setTreeFilterMode",
    ["default", "no-tools", "user-only", "labeled-only", "all"],
  ],
});

const INTEGER_SETTERS = Object.freeze({
  editorPaddingX: ["setEditorPaddingX", 0, 3],
  outputPad: ["setOutputPad", 0, 1],
  autocompleteMaxVisible: ["setAutocompleteMaxVisible", 3, 20],
});

export function piSettingsSnapshot(settings) {
  return {
    steeringMode: settings.getSteeringMode(),
    followUpMode: settings.getFollowUpMode(),
    transport: settings.getTransport(),
    compactionEnabled: settings.getCompactionEnabled(),
    retryEnabled: settings.getRetryEnabled(),
    hideThinkingBlock: settings.getHideThinkingBlock(),
    showCacheMissNotices: settings.getShowCacheMissNotices(),
    quietStartup: settings.getQuietStartup(),
    defaultProjectTrust: settings.getDefaultProjectTrust(),
    collapseChangelog: settings.getCollapseChangelog(),
    enableInstallTelemetry: settings.getEnableInstallTelemetry(),
    enableAnalytics: settings.getEnableAnalytics(),
    showImages: settings.getShowImages(),
    clearOnShrink: settings.getClearOnShrink(),
    imageAutoResize: settings.getImageAutoResize(),
    blockImages: settings.getBlockImages(),
    doubleEscapeAction: settings.getDoubleEscapeAction(),
    treeFilterMode: settings.getTreeFilterMode(),
    showHardwareCursor: settings.getShowHardwareCursor(),
    editorPaddingX: settings.getEditorPaddingX(),
    outputPad: settings.getOutputPad(),
    autocompleteMaxVisible: settings.getAutocompleteMaxVisible(),
    enableSkillCommands: settings.getEnableSkillCommands(),
  };
}

export function setPiSetting(settings, key, value) {
  if (typeof key !== "string" || key.length === 0) {
    throw invalidSetting("A Pi setting key is required");
  }

  const booleanSetter = BOOLEAN_SETTERS[key];
  if (booleanSetter) {
    settings[booleanSetter](requireBoolean(value, key));
    return;
  }

  const enumSetting = ENUM_SETTERS[key];
  if (enumSetting) {
    const [setter, options] = enumSetting;
    settings[setter](requireEnum(value, key, options));
    return;
  }

  const integerSetting = INTEGER_SETTERS[key];
  if (integerSetting) {
    const [setter, minimum, maximum] = integerSetting;
    settings[setter](requireInteger(value, key, minimum, maximum));
    return;
  }

  throw invalidSetting(`Unsupported Pi setting: ${key}`);
}
