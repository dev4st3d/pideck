import assert from "node:assert/strict";
import { Writable } from "node:stream";
import { once } from "node:events";
import { test } from "node:test";
import { createJsonlWriter } from "./jsonl.mjs";

test("a slow consumer cannot grow the write queue beyond the budget", () => {
  const errors = [];
  const stream = new Writable({ highWaterMark: 1, write(_chunk, _encoding, _done) {} });
  const writer = createJsonlWriter(stream, {
    maxRecordBytes: 32, maxBufferedBytes: 100, onFailure: code => errors.push(code),
  });
  for (let i = 0; i < 1000; i++) writer.write({ n: i });
  assert.ok(stream.writableLength <= 100);
  assert.deepEqual(errors, ["output_backpressure"]);
  assert.equal(writer.failed, true);
  stream.destroy();
});

test("drained writes preserve response ordering without a second queue", async () => {
  const records = [];
  const stream = new Writable({ write(chunk, _encoding, done) { records.push(JSON.parse(chunk)); done(); } });
  const writer = createJsonlWriter(stream);
  for (let id = 0; id < 1000; id++) assert.equal(writer.write({ id }), true);
  stream.end();
  await once(stream, "finish");
  assert.deepEqual(records.map(record => record.id), Array.from({ length: 1000 }, (_, i) => i));
});

test("record limits count encoded bytes and failure contains no payload", () => {
  const errors = [];
  const stream = new Writable({ write(_chunk, _encoding, done) { done(); } });
  const writer = createJsonlWriter(stream, {
    maxRecordBytes: 20, maxBufferedBytes: 100, onFailure: code => errors.push(code),
  });
  assert.equal(writer.write({ secret: "界".repeat(20) }), false);
  assert.deepEqual(errors, ["record_too_large"]);
  assert.equal(writer.write({ ok: true }), false);
});

test("broken output reports once instead of throwing an uncaught EPIPE", async () => {
  const errors = [];
  const stream = new Writable({ write(_chunk, _encoding, done) { done(new Error("private payload")); } });
  const writer = createJsonlWriter(stream, { onFailure: code => errors.push(code) });
  const closed = once(stream, "close").catch(() => {});
  writer.write({ id: 1 });
  await closed;
  assert.deepEqual(errors, ["output_closed"]);
  assert.equal(writer.write({ id: 2 }), false);
});
