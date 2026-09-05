/** Serialize exactly one LF-delimited JSON record. */
export function serializeJsonLine(value) {
  return `${JSON.stringify(value)}\n`;
}

/**
 * LF-only byte framing. Decode once per record, not once per incoming fragment.
 * The carry buffer grows geometrically, is bounded, and never retains a slice
 * of an arbitrarily large upstream chunk. CR is allowed only as the CRLF suffix.
 */
export function attachJsonlLineReader(
  stream,
  onLine,
  { maxRecordBytes = 1024 * 1024, onOversized } = {},
) {
  if (!Number.isSafeInteger(maxRecordBytes) || maxRecordBytes < 0) {
    throw new RangeError("maxRecordBytes must be a non-negative safe integer");
  }
  let carry = Buffer.alloc(0);
  let used = 0;
  let discarding = false;
  let attached = true;

  const emit = (bytes, start, end) => {
    if (end > start && bytes[end - 1] === 13) end--;
    if (end - start > maxRecordBytes) onOversized?.();
    else onLine(bytes.toString("utf8", start, end));
  };

  const append = (bytes, start, end) => {
    if (start === end) return;
    const nextSize = used + end - start;
    const crAllowance = end > start && bytes[end - 1] === 13 ? 1 : 0;
    if (nextSize > maxRecordBytes + crAllowance) {
      used = 0;
      discarding = true;
      onOversized?.();
      return;
    }
    if (nextSize > carry.length) {
      const capacity = Math.min(maxRecordBytes + 1, Math.max(256, carry.length * 2, nextSize));
      const next = Buffer.allocUnsafe(capacity);
      carry.copy(next, 0, 0, used);
      carry = next;
    }
    bytes.copy(carry, used, start, end);
    used = nextSize;
  };

  const onData = (chunk) => {
    const bytes = typeof chunk === "string" ? Buffer.from(chunk, "utf8") : chunk;
    let start = 0;
    while (attached && start < bytes.length) {
      const newline = bytes.indexOf(10, start);
      const end = newline < 0 ? bytes.length : newline;
      if (!discarding) {
        if (used === 0 && newline >= 0) {
          emit(bytes, start, end);
        } else {
          append(bytes, start, end);
          if (!discarding && newline >= 0) {
            // Reset before calling user code: detach and re-entry are safe.
            const length = used;
            used = 0;
            emit(carry, 0, length);
          }
        }
      }
      if (newline < 0) return;
      discarding = false;
      start = newline + 1;
    }
  };

  const detach = () => {
    attached = false;
    stream.off("data", onData);
    stream.off("end", onEnd);
    stream.off("close", detach);
    carry = Buffer.alloc(0);
    used = 0;
    discarding = false;
  };
  const onEnd = () => {
    if (attached && !discarding && used > 0) emit(carry, 0, used);
    detach();
  };
  stream.on("data", onData);
  stream.on("end", onEnd);
  stream.on("close", detach);
  return detach;
}

/**
 * Node Writable owns ordering and drain handling. Bound its actual queued bytes
 * rather than adding a second queue. Saturation is a visible transport failure,
 * never a silently dropped response or a command replay.
 */
export function createJsonlWriter(
  stream,
  { maxRecordBytes = 1024 * 1024, maxBufferedBytes = 4 * 1024 * 1024, onFailure = () => {} } = {},
) {
  if (!Number.isSafeInteger(maxRecordBytes) || maxRecordBytes < 0
      || !Number.isSafeInteger(maxBufferedBytes) || maxBufferedBytes <= maxRecordBytes) {
    throw new RangeError("Invalid JSONL writer limits");
  }
  let failed = false;
  const fail = (code) => {
    if (failed) return false;
    failed = true;
    onFailure(code);
    return false;
  };
  // A closed host pipe is not an uncaught exception carrying user payloads.
  stream.on("error", () => fail("output_closed"));
  return {
    write(value) {
      if (failed || stream.destroyed || stream.writableEnded) return fail("output_closed");
      let record;
      try {
        record = serializeJsonLine(value);
      } catch {
        return fail("invalid_output");
      }
      const bytes = Buffer.byteLength(record);
      if (bytes - 1 > maxRecordBytes) return fail("record_too_large");
      if ((stream.writableLength ?? 0) + bytes > maxBufferedBytes) return fail("output_backpressure");
      try {
        stream.write(record);
        return true;
      } catch {
        return fail("output_closed");
      }
    },
    get failed() { return failed; },
  };
}
