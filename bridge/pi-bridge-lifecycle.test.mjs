import assert from "node:assert/strict";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { join } from "node:path";
import { once } from "node:events";
import { test } from "node:test";
import { modelFixture, startFixture, waitFor } from "./test-support/harness.mjs";

function login(fixture, id, operationId = 1) {
  fixture.send({ type: "request", id, command: "login_provider",
    params: { provider: "fixture", authType: "oauth", operationId } });
}
function started(fixture, count = 1) {
  return waitFor(() => fixture.records.filter((record) => record.event === "auth_progress").length >= count,
    "provider login barrier");
}

test("SDK console output is diagnostics, never a JSONL protocol record", async (t) => {
  const fixture = startFixture(t, 'console.log("fixture extension imported"); export {};');
  await fixture.ready();
  assert.deepEqual(fixture.invalidLines, []);
  assert.match(fixture.stderr(), /fixture extension imported/);
});

test("late success after cancellation is discarded exactly once without replay", async (t) => {
  const fixture = startFixture(t, modelFixture);
  await fixture.ready();
  login(fixture, "login");
  await started(fixture);
  fixture.send({ type: "cancel", id: "cancel-1", targetId: "login" });
  assert.equal((await fixture.response("cancel-1")).result.cancelled, true);
  writeFileSync(join(fixture.root, "release.txt"), "release");
  const result = await fixture.response("login");
  assert.equal(result.ok, false);
  assert.equal(result.error.code, "cancelled");
  assert.equal(fixture.records.filter((r) => r.id === "login").length, 1);
  assert.equal(fixture.records.filter((r) => r.event === "auth_progress").length, 1);
  fixture.send({ type: "cancel", id: "cancel-2", targetId: "login" });
  assert.equal((await fixture.response("cancel-2")).result.cancelled, false);
  assert.equal((await fixture.request("still-alive", "hello")).ok, true);
});

test("duplicate active identities close the transport rather than executing twice", async (t) => {
  const fixture = startFixture(t, modelFixture);
  await fixture.ready();
  login(fixture, "same-id");
  await started(fixture);
  login(fixture, "same-id", 2);
  await waitFor(fixture.exit, "duplicate ID fatal shutdown");
  assert.equal(fixture.exit().code, 1);
  assert.match(fixture.stderr(), /duplicate_request_id/);
  assert.equal(fixture.records.filter((r) => r.event === "auth_progress").length, 1);
});

test("request admission is bounded while cancellation and auth replies retain access", async (t) => {
  const fixture = startFixture(t, modelFixture);
  await fixture.ready();
  for (let index = 0; index < 63; index++) login(fixture, `login-${index}`, index);
  await started(fixture, 63);
  assert.equal((await fixture.request("full", "hello")).error.code, "busy");
  const auth = await fixture.request("auth-reserved", "auth_respond", {
    operationId: 0, promptId: "absent", value: "fixture-value",
  });
  assert.notEqual(auth.error?.code, "busy");
  for (let index = 0; index < 63; index++) {
    fixture.send({ type: "cancel", id: `cancel-${index}`, targetId: `login-${index}` });
  }
  assert.equal((await fixture.response("cancel-62")).result.cancelled, true);
  writeFileSync(join(fixture.root, "release.txt"), "release");
  await Promise.all(Array.from({ length: 63 }, (_, i) => fixture.response(`login-${i}`)));
  assert.equal((await fixture.request("capacity-restored", "hello")).ok, true);
});

test("failed model initialization can be retried without retaining the rejected promise", async (t) => {
  const fixture = startFixture(t, modelFixture);
  writeFileSync(join(fixture.root, "fail-first.txt"), "fixture flag");
  await fixture.ready();
  const first = await fixture.request("model-1", "get_model_runtime");
  assert.equal(first.ok, false);
  assert.doesNotMatch(JSON.stringify(first), /SENSITIVE/);
  const second = await fixture.request("model-2", "get_model_runtime");
  assert.equal(second.ok, true);
  assert.deepEqual(second.result.models, []);
  assert.equal(readFileSync(join(fixture.root, "creates.txt"), "utf8"), "2");
});

test("JSONL exports are exclusive and cannot overwrite a session or previous export", async (t) => {
  const fixture = startFixture(t, String.raw`
    export const SessionManager = { open() { return {
      getSessionId: () => "fixture-session", getCwd: () => process.cwd(),
      getBranch: () => [{ id: "first", parentId: null, type: "message", message: {role: "user", content: "hello"} }],
    }; } };
  `);
  await fixture.ready();
  const source = join(fixture.root, "session.jsonl");
  const output = join(fixture.root, "export.jsonl");
  writeFileSync(source, "original session bytes\n");
  const params = { sessionPath: source, cwd: fixture.root, outputPath: output };
  assert.equal((await fixture.request("export-1", "export_jsonl", params)).ok, true);
  const bytes = readFileSync(output);
  assert.equal(JSON.parse(bytes.toString().split("\n")[0]).id, "fixture-session");
  assert.equal((await fixture.request("export-2", "export_jsonl", params)).error.code, "output_exists");
  assert.deepEqual(readFileSync(output), bytes);
  assert.equal((await fixture.request("export-source", "export_jsonl", { ...params, outputPath: source })).error.code,
    "output_exists");
  assert.equal(readFileSync(source, "utf8"), "original session bytes\n");
  if (process.platform !== "win32") assert.equal(statSync(output).mode & 0o777, 0o600);
});

test("stdin EOF closes unhandshaken sockets and removes the owned endpoint", async (t) => {
  const fixture = startFixture(t, "export {};", { orchestration: true });
  await fixture.ready();
  if (process.platform !== "win32") await waitFor(() => existsSync(fixture.endpoint), "socket path");
  const socket = createConnection(fixture.endpoint);
  socket.on("error", () => {});
  const closed = once(socket, "close");
  await once(socket, "connect");
  fixture.child.stdin.end();
  await waitFor(fixture.exit, "EOF exit");
  await closed;
  assert.equal(fixture.exit().code, 0);
  if (process.platform !== "win32") assert.equal(existsSync(fixture.endpoint), false);
});
