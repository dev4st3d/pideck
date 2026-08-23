import assert from "node:assert/strict";
import { EventEmitter, once } from "node:events";
import { PassThrough } from "node:stream";
import { test } from "node:test";

import { attachJsonlLineReader, createJsonlWriter, serializeJsonLine } from "./jsonl.mjs";

test("strict JSONL preserves Unicode line and paragraph separators", () => {
  const stream = new PassThrough();
  const lines = [];
  attachJsonlLineReader(stream, (line) => lines.push(line));

  const first = { text: "alpha\u2028beta\u2029gamma" };
  stream.end(`${serializeJsonLine(first)}${serializeJsonLine({ ok: true })}`);

  assert.deepEqual(lines.map(JSON.parse), [first, { ok: true }]);
});

test("strict JSONL reconstructs UTF-8 split across stream chunks", () => {
  const stream = new PassThrough();
  const lines = [];
  attachJsonlLineReader(stream, (line) => lines.push(line));

  const encoded = Buffer.from(serializeJsonLine({ text: "before 🚀 after" }), "utf8");
  const rocket = Buffer.from("🚀", "utf8");
  const split = encoded.indexOf(rocket) + 2;
  stream.write(encoded.subarray(0, split));
  stream.end(encoded.subarray(split));

  assert.deepEqual(lines.map(JSON.parse), [{ text: "before 🚀 after" }]);
});

test("strict JSONL accepts CRLF and a final unterminated record", async () => {
  const stream = new PassThrough();
  const lines = [];
  attachJsonlLineReader(stream, (line) => lines.push(line));

  stream.end('{"a":1}\r\n{"b":2}');
  await once(stream, "end");

  assert.deepEqual(lines, ['{"a":1}', '{"b":2}']);
});

test("oversized records are discarded without losing the following record", () => {
  const stream = new PassThrough();
  const lines = [];
  let oversized = 0;
  attachJsonlLineReader(stream, (line) => lines.push(line), {
    maxRecordBytes: 12,
    onOversized: () => oversized++,
  });

  stream.write('x'.repeat(40));
  stream.end('\n{"ok":true}\n');

  assert.equal(oversized, 1);
  assert.deepEqual(lines, ['{"ok":true}']);
});

class BackpressuredSink extends EventEmitter {
  constructor(writeResults) {
    super();
    this.writeResults = [...writeResults];
    this.lines = [];
  }

  write(line) {
    this.lines.push(line);
    return this.writeResults.length > 0 ? this.writeResults.shift() : true;
  }
}

test("JSONL writer preserves record order across backpressure", () => {
  const sink = new BackpressuredSink([false, true, true]);
  const writer = createJsonlWriter(sink);

  assert.equal(writer.write({ sequence: 1 }), true);
  assert.equal(writer.write({ sequence: 2 }), true);
  assert.equal(writer.write({ sequence: 3 }), true);
  assert.deepEqual(sink.lines.map(JSON.parse), [{ sequence: 1 }]);

  sink.emit("drain");

  assert.deepEqual(sink.lines.map(JSON.parse), [
    { sequence: 1 },
    { sequence: 2 },
    { sequence: 3 },
  ]);
});

test("JSONL writer bounds queued output while a stream is blocked", () => {
  const sink = new BackpressuredSink([false]);
  const overflows = [];
  const writer = createJsonlWriter(sink, {
    maxBufferedBytes: 28,
    onOverflow: (details) => overflows.push(details),
  });

  assert.equal(writer.write({ a: 1 }), true);
  assert.equal(writer.write({ payload: "12345678" }), true);
  assert.equal(writer.write({ payload: "abcdefgh" }), false);
  assert.equal(writer.write({ ignored: true }), false);
  assert.equal(overflows.length, 1);
  assert.equal(sink.lines.length, 1);
});
