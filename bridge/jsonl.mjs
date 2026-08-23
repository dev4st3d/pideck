import { StringDecoder } from "node:string_decoder";

/** Serialize exactly one LF-delimited JSON record. */
export function serializeJsonLine(value) {
  return `${JSON.stringify(value)}\n`;
}

/**
 * Create an ordered JSONL writer that honors Node stream backpressure.
 *
 * A blocked stdout/socket must not be written repeatedly: doing so moves the
 * queue into Node's internal buffer with no application-level bound. This
 * writer keeps one bounded FIFO until `drain`, preserving event/response order
 * across independent producers while failing closed on pathological output.
 */
export function createJsonlWriter(
  stream,
  {
    maxBufferedBytes = 4 * 1024 * 1024,
    onOverflow,
  } = {},
) {
  let queue = [];
  let queueHead = 0;
  let bufferedBytes = 0;
  let blocked = false;
  let closed = false;
  let overflowed = false;

  const compactQueue = () => {
    if (queueHead === 0) return;
    if (queueHead >= queue.length) {
      queue = [];
      queueHead = 0;
      return;
    }
    if (queueHead >= 64 && queueHead * 2 >= queue.length) {
      queue = queue.slice(queueHead);
      queueHead = 0;
    }
  };

  const overflow = (nextBytes) => {
    if (!overflowed) {
      overflowed = true;
      onOverflow?.({ bufferedBytes, nextBytes, maxBufferedBytes });
    }
    return false;
  };

  const flush = () => {
    if (closed) return;
    blocked = false;
    while (queueHead < queue.length) {
      const entry = queue[queueHead++];
      bufferedBytes -= entry.bytes;
      if (!stream.write(entry.line)) {
        blocked = true;
        stream.once("drain", flush);
        compactQueue();
        return;
      }
    }
    compactQueue();
  };

  const writeLine = (line) => {
    if (closed || overflowed) return false;
    const bytes = Buffer.byteLength(line, "utf8");
    if (bytes > maxBufferedBytes) return overflow(bytes);

    if (blocked) {
      if (bufferedBytes + bytes > maxBufferedBytes) return overflow(bytes);
      queue.push({ line, bytes });
      bufferedBytes += bytes;
      return true;
    }

    if (!stream.write(line)) {
      blocked = true;
      stream.once("drain", flush);
    }
    return true;
  };

  return {
    write(value) {
      return writeLine(serializeJsonLine(value));
    },
    close() {
      if (closed) return;
      closed = true;
      stream.off?.("drain", flush);
      queue = [];
      queueHead = 0;
      bufferedBytes = 0;
    },
  };
}

/**
 * Attach a strict LF-only JSONL reader.
 *
 * Node's readline also treats U+2028/U+2029 as record separators. Those
 * characters are valid inside JSON strings, so using readline can split a
 * valid record in the middle. This reader only splits on `\n`, accepts an
 * optional trailing `\r`, and bounds incomplete records without retaining
 * unbounded input.
 */
export function attachJsonlLineReader(
  stream,
  onLine,
  { maxRecordBytes = Number.POSITIVE_INFINITY, onOversized } = {},
) {
  const decoder = new StringDecoder("utf8");
  let buffer = "";
  let discardingOversizedRecord = false;

  const emitLine = (line) => {
    const normalized = line.endsWith("\r") ? line.slice(0, -1) : line;
    if (Buffer.byteLength(normalized, "utf8") > maxRecordBytes) {
      onOversized?.();
      return;
    }
    onLine(normalized);
  };

  const consume = () => {
    while (true) {
      if (discardingOversizedRecord) {
        const newlineIndex = buffer.indexOf("\n");
        if (newlineIndex === -1) {
          buffer = "";
          return;
        }
        buffer = buffer.slice(newlineIndex + 1);
        discardingOversizedRecord = false;
        continue;
      }

      const newlineIndex = buffer.indexOf("\n");
      if (newlineIndex >= 0) {
        emitLine(buffer.slice(0, newlineIndex));
        buffer = buffer.slice(newlineIndex + 1);
        continue;
      }

      if (Buffer.byteLength(buffer, "utf8") > maxRecordBytes) {
        buffer = "";
        discardingOversizedRecord = true;
        onOversized?.();
      }
      return;
    }
  };

  const onData = (chunk) => {
    buffer += typeof chunk === "string" ? chunk : decoder.write(chunk);
    consume();
  };

  const onEnd = () => {
    buffer += decoder.end();
    if (!discardingOversizedRecord && buffer.length > 0) emitLine(buffer);
    buffer = "";
    discardingOversizedRecord = false;
  };

  stream.on("data", onData);
  stream.on("end", onEnd);

  return () => {
    stream.off("data", onData);
    stream.off("end", onEnd);
    buffer = "";
    discardingOversizedRecord = false;
  };
}
