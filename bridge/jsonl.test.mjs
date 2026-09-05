import assert from "node:assert/strict";
import { once } from "node:events";
import { PassThrough } from "node:stream";
import { test } from "node:test";

import { attachJsonlLineReader, serializeJsonLine } from "./jsonl.mjs";

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

test("every UTF-8 byte boundary and mixed CRLF framing produce the same records", () => {
  const expected = [{ text: "x🙂界\u2028\u2029é" }, { n: 2 }];
  const bytes = Buffer.from(`${JSON.stringify(expected[0])}\r\n${JSON.stringify(expected[1])}\n`);
  for (let split = 0; split <= bytes.length; split++) {
    const stream = new PassThrough();
    const lines = [];
    attachJsonlLineReader(stream, line => lines.push(JSON.parse(line)));
    stream.write(bytes.subarray(0, split));
    stream.end(bytes.subarray(split));
    assert.deepEqual(lines, expected, `split ${split}`);
  }
});

test("the byte limit excludes CRLF and is independent of fragmentation", () => {
  for (const size of [1, 2, 3, 7, 100]) {
    const stream = new PassThrough();
    const lines = [];
    let oversized = 0;
    attachJsonlLineReader(stream, line => lines.push(line), {
      maxRecordBytes: 3, onOversized: () => oversized++,
    });
    const bytes = Buffer.from("abc\r\n界\nxxxx\nabc\rX\nZ\n");
    for (let offset = 0; offset < bytes.length; offset += size) {
      stream.write(bytes.subarray(offset, offset + size));
    }
    stream.end();
    assert.deepEqual(lines, ["abc", "界", "Z"]);
    assert.equal(oversized, 2);
  }
});

test("an oversized record is reported once even across many fragments", async () => {
  const stream = new PassThrough();
  const lines = [];
  let oversized = 0;
  attachJsonlLineReader(stream, line => lines.push(line), {
    maxRecordBytes: 16, onOversized: () => oversized++,
  });
  for (let i = 0; i < 10000; i++) stream.write(Buffer.alloc(17, 120));
  stream.end("\n{}\n");
  await once(stream, "end");
  assert.deepEqual(lines, ["{}"]);
  assert.equal(oversized, 1);
});

test("detach inside a callback prevents delivery of the rest of the chunk", () => {
  const stream = new PassThrough();
  const lines = [];
  const detach = attachJsonlLineReader(stream, line => { lines.push(line); detach(); });
  stream.write("first\nsecond\n");
  assert.deepEqual(lines, ["first"]);
  assert.equal(stream.listenerCount("data"), 0);
});

test("abrupt close discards a partial record, while EOF emits it exactly once", async () => {
  const stream = new PassThrough();
  const lines = [];
  attachJsonlLineReader(stream, line => lines.push(line));
  stream.write("partial");
  stream.destroy();
  await once(stream, "close");
  assert.deepEqual(lines, []);
  const complete = new PassThrough();
  attachJsonlLineReader(complete, line => lines.push(line));
  complete.end("tail");
  await once(complete, "end");
  assert.deepEqual(lines, ["tail"]);
});

test("empty lines, zero limits, and a final CR are handled consistently", async () => {
  const stream = new PassThrough();
  const lines = [];
  let oversized = 0;
  attachJsonlLineReader(stream, line => lines.push(line), {
    maxRecordBytes: 0, onOversized: () => oversized++,
  });
  stream.end("\n\r\nx\n\r");
  await once(stream, "end");
  assert.deepEqual(lines, ["", "", ""]);
  assert.equal(oversized, 1);
});

test("invalid framing limits fail immediately", () => {
  for (const value of [-1, 1.5, NaN, Infinity]) {
    assert.throws(() => attachJsonlLineReader(new PassThrough(), () => {}, {
      maxRecordBytes: value,
    }), RangeError);
  }
});
