import { StringDecoder } from "node:string_decoder";

/** Serialize exactly one LF-delimited JSON record. */
export function serializeJsonLine(value) {
  return `${JSON.stringify(value)}\n`;
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
