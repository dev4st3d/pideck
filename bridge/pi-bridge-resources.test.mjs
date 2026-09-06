import assert from "node:assert/strict";
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { startFixture, waitFor } from "./test-support/harness.mjs";

const resourceFixture = String.raw`
import assert from "node:assert/strict";
import { appendFileSync, existsSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
let loaders = 0;
let sessions = 0;
export const getAgentDir = () => resolve("global-agent");
function settings(projectTrusted) {
  return { projectTrusted, drainErrors: () => [], getEnableSkillCommands: () => true,
    getThemeSetting: () => "dark", getDefaultProjectTrust: () => "never" };
}
export const SettingsManager = {
  create(_cwd, _agent, options) { return settings(options.projectTrusted); },
  inMemory(_values, options) { assert.equal(options.projectTrusted, false); return settings(false); },
};
export class DefaultPackageManager {
  constructor(options) { this.trusted = options.settingsManager.projectTrusted; }
  async resolve(install) {
    assert.equal(await install({}), "skip");
    const global = { path: resolve("global-extension.mjs"), enabled: true, metadata: { scope: "user" } };
    const project = { path: resolve("project-extension.mjs"), enabled: true, metadata: { scope: "project" } };
    return { extensions: this.trusted ? [global, project] : [global], skills: [], prompts: [], themes: [] };
  }
  listConfiguredPackages() { return []; }
}
export class DefaultResourceLoader {
  constructor(options) {
    this.id = ++loaders;
    for (const key of ["noExtensions", "noSkills", "noPromptTemplates", "noThemes", "noContextFiles"])
      assert.equal(options[key], true);
    assert.deepEqual(options.additionalExtensionPaths, [resolve("global-extension.mjs")]);
    writeFileSync("loader-options-" + this.id + ".json", JSON.stringify(options));
  }
  async reload() {
    writeFileSync("loader-start-" + this.id, "started");
    if (existsSync("fail-load")) throw new Error("sensitive-resource-diagnostics");
    if (existsSync("hold-load")) await new Promise((resolvePromise) => {
      const timer = setInterval(() => {
        if (existsSync("release-load")) { clearInterval(timer); resolvePromise(); }
      }, 5);
    });
  }
  getExtensions() { return { extensions: [], errors: [] }; }
  getSkills() { return { skills: [], diagnostics: [] }; }
  getPrompts() { return { prompts: [], diagnostics: [] }; }
  getThemes() { return { themes: [], diagnostics: [] }; }
}
export const ModelRuntime = { create: async () => ({}) };
export const SessionManager = { inMemory: () => ({}) };
export async function createAgentSession(options) {
  assert.equal(options.settingsManager.projectTrusted, false);
  const id = ++sessions;
  writeFileSync("session-" + id, "created");
  return { session: {
    getActiveToolNames: () => [], getAllTools: () => [],
    dispose() { appendFileSync("disposed", id + "\n"); },
  } };
}
export const loadProjectContextFiles = () => [];
`;

test("cancelled and failed reloads retain the last valid resource plane and project trust", async (t) => {
  const fixture = startFixture(t, resourceFixture);
  await fixture.ready();
  const initial = await fixture.request("inventory", "get_resource_inventory");
  assert.equal(initial.ok, true);
  assert.equal(initial.result.generation, 1);
  assert.equal(initial.result.projectTrusted, false);
  const project = initial.result.items.find((item) => item.name === "project-extension.mjs");
  assert.equal(project.state, "disabled");
  assert.equal(project.trust, "rejected");
  writeFileSync(join(fixture.root, "hold-load"), "hold");
  fixture.send({ type: "request", id: "reload-cancelled", command: "reload_resources", params: {} });
  await waitFor(() => existsSync(join(fixture.root, "loader-start-2")), "reload reached SDK");
  fixture.send({ type: "cancel", id: "cancel", targetId: "reload-cancelled" });
  assert.equal((await fixture.response("cancel")).result.cancelled, true);
  fixture.send({ type: "request", id: "other-reader", command: "get_resource_inventory", params: {} });
  writeFileSync(join(fixture.root, "release-load"), "release");
  assert.equal((await fixture.response("reload-cancelled")).error.code, "cancelled");
  const retained = await fixture.response("other-reader");
  assert.equal(retained.ok, true);
  assert.deepEqual(retained.result, initial.result);
  assert.equal(existsSync(join(fixture.root, "disposed")), false);
  unlinkSync(join(fixture.root, "hold-load"));
  writeFileSync(join(fixture.root, "fail-load"), "fail");
  const failed = await fixture.request("reload-failed", "reload_resources");
  assert.equal(failed.ok, false);
  assert.doesNotMatch(JSON.stringify(failed), /sensitive-resource/);
  assert.deepEqual((await fixture.request("after-failure", "get_resource_inventory")).result, initial.result);
  assert.equal(existsSync(join(fixture.root, "disposed")), false);
  unlinkSync(join(fixture.root, "fail-load"));
  const replacement = await fixture.request("reload-good", "reload_resources");
  assert.equal(replacement.ok, true);
  assert.equal(replacement.result.generation, 2);
  assert.equal(readFileSync(join(fixture.root, "disposed"), "utf8"), "1\n");
  assert.deepEqual(fixture.records.filter((record) => record.event === "resources_changed")
    .map((record) => record.generation), [1, 2]);
});


test("inventory preserves all tools and providers exported from the same extension", async (t) => {
  const exportsFixture = resourceFixture
    .replace('getExtensions() { return { extensions: [], errors: [] }; }', `getExtensions() {
      return { extensions: [], errors: [], runtime: {
        pendingProviderRegistrations: [
          { name: "provider-first", extensionPath: resolve("global-extension.mjs") },
          { name: "provider-second", extensionPath: resolve("global-extension.mjs") },
        ],
      } };
    }`)
    .replace('getActiveToolNames: () => [], getAllTools: () => [],', `
      getActiveToolNames: () => ["tool-second"],
      getAllTools: () => ["tool-first", "tool-second"].map((name) => ({
        name, description: "synthetic tool", sourceInfo: { path: resolve("global-extension.mjs") },
      })),`);
  const fixture = startFixture(t, exportsFixture);
  await fixture.ready();
  const response = await fixture.request("multi-export-inventory", "get_resource_inventory");
  assert.equal(response.ok, true);
  const tools = response.result.items.filter((row) => row.kind === "tool");
  const providers = response.result.items.filter((row) => row.kind === "provider");
  assert.deepEqual(tools.map((row) => row.name), ["tool-first", "tool-second"]);
  assert.deepEqual(providers.map((row) => row.name), ["provider-first", "provider-second"]);
  assert.deepEqual(tools.map((row) => row.active), [false, true]);
  assert.equal(new Set([...tools, ...providers].map((row) => row.id)).size, 4);
});
