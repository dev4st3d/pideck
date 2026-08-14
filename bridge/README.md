# Pi SDK sidecar contract

`pi-bridge.mjs` is the only Node process used for public Pi SDK capabilities that stock RPC does not expose. It inherits stdin, stdout, and stderr from the Rust supervisor and never binds or listens on a network port. The bridge and Pi SDK objects remain inside the Pi child/sidecar trust boundary.

Release builds embed every runtime bridge module in `pi-gui.exe`. Rust materializes a content-addressed copy in the user's temporary directory because Node's ESM loader and Pi's extension loader require real file paths; release artifacts do not include a separate `bridge` folder.

## Wire ownership

- Rust owns discovery, process start/stop/restart, request IDs, deadlines, cancellation requests, capability checks, stale-result rejection, and user-visible state.
- Node owns Pi SDK objects, SDK version-sensitive calls, resource loading, secret redaction at the SDK boundary, and clean disposal when stdin closes.
- Records are bounded UTF-8 JSONL with LF as the only record delimiter. The shared `jsonl.mjs` reader preserves U+2028/U+2029 inside JSON strings, accepts optional CRLF input, and drops oversized incomplete records without retaining unbounded memory. `protocol.ts` documents the TypeScript shapes and `protocol.schema.json` is the machine-readable v1 request/response/event contract.
- Changes are additive within protocol v1. A command is usable only when both the negotiated hello capability and the Node command gate allow it. Unknown record versions fail with `incompatible_protocol`.

## Cancellation and restart

Cancellation is correlated by request ID. Operations with an SDK abort surface receive an `AbortSignal`; branch summaries also call Pi's abort API. Resource loading is not synchronously interruptible in Pi 0.84.2, so cancellation marks its result stale and Rust keeps the prior valid inventory. Restart is the recovery boundary for an extension that does not return: Rust closes/kills the sidecar, rejects pending requests, starts a fresh process, renegotiates hello, and reloads snapshots. No prompt or session operation is replayed automatically.

Closing stdin aborts active controllers, aborts an active branch summary, disposes the resource-plane session, and lets Node exit. Rust also waits after forced termination, so the sidecar cannot outlive its owner during normal shutdown.

## Orchestration adapter

`orchestration-adapter.mjs` is injected into the same Pi extension process as the installed `pi-tasks`, `pi-subagents`, and `pi-goal` extensions. It reads task files through the extension's `TaskStore`, subagents through the extension's process-global manager and lifecycle event bus, schedules from the extension's session store, and goals from canonical `goal-state` session entries. It never parses TUI widgets or derives semantic state from transcript tool names.

The adapter connects to `pi-bridge.mjs` over a per-process local named pipe/Unix socket. The sidecar proxies bounded typed snapshots, lifecycle events, and correlated actions to Rust; the endpoint is not a network listener. Task execution preserves dependency and cycle guards before using the subagent RPC bus. Subagent actions verify current IDs and lifecycle state. Goal snapshots include pi-goal's wait state, automatic-response and no-progress counters, safety-pause cause, and ordered-queue freeze state. Goal actions verify the active goal ID and reject edits or resume attempts while an ordered queue is frozen, then return the installed extension command for invocation through Pi so its completion, blocking, budget, pause, and resume guards remain authoritative.

The last valid snapshot remains visible as stale after adapter loss. A short disconnect grace period hides harmless sidecar replacement flicker, and reconnect attempts use bounded exponential backoff instead of a permanent 500 ms retry loop. Every adapter process handshakes with a `producerId`, so generation ordering survives extension reloads where the local generation counter restarts. The bridge retires superseded producers, while socket identity fencing prevents buffered or reconnecting records from an old adapter from regressing the current snapshot. Bridge restart establishes a fresh endpoint and reloads the active session; a session switch clears the old snapshot until the new session ID is observed.

Task completion is deliberately conservative: only the subagent extension's successful terminal states (`completed` and `steered`) mark a task complete and unlock dependents. A stopped, aborted, or failed agent returns its task to `pending` with a bounded error note; it never cascades dependents as though the work succeeded. Runtime metadata resets use the task store's documented null tombstones, and its empty-string owner clear value is hidden from snapshots, so retries cannot inherit a stale agent ID, result, error, or visible owner. On session start, persisted `in_progress` tasks are matched against the live subagent manager; orphaned runs are reset to retryable `pending` state. Session shutdown force-resets every still-running task before Pi aborts its in-memory agents.

## Resource and trust policy

The resource plane uses only Pi 0.84.2 public SDK exports. Already-installed global resources are loaded into a disposable in-memory SDK session so tool inventory and active-tool state come from `getAllTools()` and `getActiveToolNames()`. Project settings are inspected through `SettingsManager` and `DefaultPackageManager` only to build provenance; project extensions and package code are never passed to the loader because Pi GUI fixes project trust to rejected.

Every package resolution supplies Pi's explicit `skip` callback for missing sources. Therefore inventory and reload cannot install a package. Package install, remove, update, and configuration commands are capability-gated off until the GUI has explicit arbitrary-code confirmation, progress, pin/filter handling, and rollback-safe errors.

Context contents, prompt contents, tool schemas, credentials, resolved environment values, headers, base URLs, raw extension errors, and raw settings errors are not returned. Intended source paths and bounded descriptive metadata are returned. Failures use stable redacted codes/messages; the Resource Center can retain the last valid snapshot after an error.
