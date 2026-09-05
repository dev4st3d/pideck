import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { test } from "node:test";
import { resolveSdkEntry, PI_VERSION } from "./pi-contract.mjs";
import { manifest, startFixture, waitFor } from "./test-support/harness.mjs";

test("public SDK identity, exact version and package export are checked together", (t) => {
  const root = mkdtempSync(join(tmpdir(), "pideck contract 界 "));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "dist"));
  writeFileSync(join(root, "dist", "index.js"), "export {};\n");
  const set = (value) => writeFileSync(join(root, "package.json"), JSON.stringify(value));
  set(manifest);
  assert.deepEqual(resolveSdkEntry(root), {
    version: PI_VERSION, entry: pathToFileURL(join(root, "dist", "index.js")).href,
  });
  for (const change of [
    { name: "different-package" }, { version: "0.84.2" }, { version: "0.85.2" },
    { version: "0.85.1-beta.1" }, { bin: { pi: "dist/cli.js" } },
    { exports: { ".": { source: "./src/index.ts" } } },
    { exports: { ".": { import: "./dist/internal.js" } } },
  ]) {
    set({ ...manifest, ...change });
    assert.throws(() => resolveSdkEntry(root), /Requires/);
  }
});

test("a public export symlink cannot escape its package", (t) => {
  const root = mkdtempSync(join(tmpdir(), "pideck export "));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const sdkRoot = join(root, "sdk");
  mkdirSync(join(sdkRoot, "dist"), { recursive: true });
  writeFileSync(join(sdkRoot, "package.json"), JSON.stringify(manifest));
  writeFileSync(join(root, "outside.mjs"), "export {};\n");
  try { symlinkSync(join(root, "outside.mjs"), join(sdkRoot, "dist", "index.js")); }
  catch (error) {
    if (["EPERM", "EACCES"].includes(error.code)) { t.skip("Symlink permission unavailable"); return; }
    throw error;
  }
  assert.throws(() => resolveSdkEntry(sdkRoot), /within its package/);
});

test("incompatible package code is never evaluated", async (t) => {
  const fixture = startFixture(t,
    'import {writeFileSync} from "node:fs"; writeFileSync("imported.txt", "unsafe");',
    { manifest: { version: "0.84.2" } });
  await waitFor(fixture.exit, "rejected package exit");
  assert.equal(fixture.exit().code, 2);
  assert.equal(existsSync(join(fixture.root, "imported.txt")), false);
  assert.deepEqual(fixture.records, []);
});
