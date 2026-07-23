# Pi SDK sidecar contract

`pi-bridge.mjs` is the only Node process used for public Pi SDK capabilities that stock RPC does not expose. It inherits stdin, stdout, and stderr from the Rust supervisor and never binds or listens on a network port. The bridge and Pi SDK objects remain inside the Pi child/sidecar trust boundary.

## Wire ownership

- Rust owns discovery, process start/stop/restart, request IDs, deadlines, cancellation requests, capability checks, stale-result rejection, and user-visible state.
- Node owns Pi SDK objects, SDK version-sensitive calls, resource loading, secret redaction at the SDK boundary, and clean disposal when stdin closes.
- Records are bounded UTF-8 JSONL. `protocol.ts` documents the TypeScript shapes and `protocol.schema.json` is the machine-readable v1 request/response/event contract.
- Changes are additive within protocol v1. A command is usable only when both the negotiated hello capability and the Node command gate allow it. Unknown record versions fail with `incompatible_protocol`.

## Cancellation and restart

Cancellation is correlated by request ID. Operations with an SDK abort surface receive an `AbortSignal`; branch summaries also call Pi's abort API. Resource loading is not synchronously interruptible in Pi 0.80.10, so cancellation marks its result stale and Rust keeps the prior valid inventory. Restart is the recovery boundary for an extension that does not return: Rust closes/kills the sidecar, rejects pending requests, starts a fresh process, renegotiates hello, and reloads snapshots. No prompt or session operation is replayed automatically.

Closing stdin aborts active controllers, aborts an active branch summary, disposes the resource-plane session, and lets Node exit. Rust also waits after forced termination, so the sidecar cannot outlive its owner during normal shutdown.

## Resource and trust policy

The resource plane uses only Pi 0.80.10 public SDK exports. Already-installed global resources are loaded into a disposable in-memory SDK session so tool inventory and active-tool state come from `getAllTools()` and `getActiveToolNames()`. Project settings are inspected through `SettingsManager` and `DefaultPackageManager` only to build provenance; project extensions and package code are never passed to the loader because Pi GUI fixes project trust to rejected.

Every package resolution supplies Pi's explicit `skip` callback for missing sources. Therefore inventory and reload cannot install a package. Package install, remove, update, and configuration commands are capability-gated off until the GUI has explicit arbitrary-code confirmation, progress, pin/filter handling, and rollback-safe errors.

Context contents, prompt contents, tool schemas, credentials, resolved environment values, headers, base URLs, raw extension errors, and raw settings errors are not returned. Intended source paths and bounded descriptive metadata are returned. Failures use stable redacted codes/messages; the Resource Center can retain the last valid snapshot after an error.
