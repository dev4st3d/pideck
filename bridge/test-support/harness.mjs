import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { attachJsonlLineReader, serializeJsonLine } from "../jsonl.mjs";
import { PI_PACKAGE, PI_VERSION } from "../pi-contract.mjs";

export const manifest = {
  name: PI_PACKAGE, version: PI_VERSION, type: "module",
  bin: { pi: "dist/bundle/cli.js" },
  exports: { ".": { import: "./dist/index.js" } },
};

export async function waitFor(read, message = "condition", timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = read();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`Timed out: ${message}`);
}

export function startFixture(t, source, options = {}) {
  const root = mkdtempSync(join(tmpdir(), "pideck fixture 界 "));
  const sdkRoot = join(root, "sdk with spaces");
  mkdirSync(join(sdkRoot, "dist"), { recursive: true });
  writeFileSync(join(sdkRoot, "package.json"), JSON.stringify({ ...manifest, ...options.manifest }));
  writeFileSync(join(sdkRoot, "dist", "index.js"), source);
  const endpoint = process.platform === "win32"
    ? `\\\\.\\pipe\\pideck-fixture-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`
    : join(root, "orchestra.sock");
  const child = spawn(process.execPath, [
    fileURLToPath(new URL("../pi-bridge.mjs", import.meta.url)), sdkRoot,
  ], {
    cwd: root, stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, PI_GUI_ORCHESTRATION_PIPE: options.orchestration ? endpoint : "" },
  });
  const records = [];
  const invalidLines = [];
  let stderr = "";
  let exit;
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (text) => { stderr = (stderr + text).slice(-8192); });
  child.stdin.on("error", () => {});
  child.on("error", (error) => { exit = { error }; });
  child.on("exit", (code, signal) => { exit = { code, signal }; });
  const detach = attachJsonlLineReader(child.stdout, (line) => {
    try { records.push(JSON.parse(line)); } catch { invalidLines.push(line); }
  });
  t.after(async () => {
    if (!exit) child.stdin.end();
    try { await waitFor(() => exit, "fixture exit", 1800); }
    catch {
      child.kill("SIGKILL");
      await waitFor(() => exit, "forced fixture exit");
    }
    detach();
    rmSync(root, { recursive: true, force: true });
  });
  function send(record) { child.stdin.write(serializeJsonLine({ version: 1, ...record })); }
  return {
    root, sdkRoot, endpoint, child, records, invalidLines, send,
    stderr: () => stderr,
    exit: () => exit,
    response: (id) => waitFor(() => records.find((r) => r.type === "response" && r.id === id), id),
    async request(id, command, params = {}) {
      send({ type: "request", id, command, params });
      return this.response(id);
    },
    async ready() {
      const hello = await this.request("hello", "hello");
      assert.equal(hello.result?.sdkVersion, PI_VERSION);
      return hello;
    },
  };
}

// Synthetic public SDK methods only. This fixture is deliberately NOT evidence
// of running a real Pi installation or making a provider/network request.
export const modelFixture = String.raw`
import { existsSync, writeFileSync } from "node:fs";
let creates = 0;
export const getAgentDir = () => ".pi";
export const SettingsManager = {
  create() {
    return new Proxy({}, {
      get(_target, key) {
        if (key === "getEnabledModels" || key === "drainErrors") return () => [];
        if (key === "flush") return async () => {};
        return () => undefined;
      },
    });
  },
};
export const ModelRuntime = {
  async create() {
    creates += 1;
    writeFileSync("creates.txt", String(creates));
    if (existsSync("fail-first.txt") && creates === 1) throw new Error("SENSITIVE-init-details");
    return {
      getModels: () => [], getAvailableSnapshot: () => [], getProviders: () => [],
      getError: () => undefined,
      async login(_provider, _type, interaction) {
        interaction.notify({ type: "progress", message: "fixture-login-started" });
        await new Promise((resolve) => {
          const timer = setInterval(() => {
            if (existsSync("release.txt")) { clearInterval(timer); resolve(); }
          }, 5);
        });
      },
    };
  },
};
`;
