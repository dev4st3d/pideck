# GPUI and Pi reliability pass

This change set targets the two symptoms that were coupled in practice: slow GPUI frames and Pi/subagent reconnect loops. It avoids a framework rewrite and fixes the pressure, lifecycle, and invalidation boundaries that made ordinary streaming load look like process failure.

## Root causes addressed

### 1. Development builds were running the hot UI path unoptimized

The project had no development profile, so `cargo run` used Rust's default unoptimized code for GPUI layout, JSON decoding, markdown parsing, and image work. `Cargo.toml` now keeps debuggable application code while optimizing the renderer-facing dependencies. Release builds also use one codegen unit with thin LTO.

This follows the same practical pattern used by Zed: retain a useful development profile while selectively optimizing GPUI-adjacent hot dependencies.

### 2. Streaming events caused redundant app-wide invalidations

The controller now:

- dispatches replaceable token/tool updates at most once per 16 ms interval;
- keeps only the newest update for each message/tool inside a batch while preserving the order of the retained events;
- never coalesces across lifecycle or control records;
- accepts substantially larger bounded notification queues before applying backpressure;
- avoids updating transcript text entities when their source hash is unchanged.

The conversation remains virtualized. These changes reduce the work feeding that virtual list and remove no-op entity notifications.

### 3. A full stdout queue incorrectly killed Pi

The process supervisor previously treated a temporarily full bounded stdout queue as a fatal `StdoutBackpressure` error and terminated Pi. That is especially likely when a slow UI frame delays the RPC consumer.

The stdout worker now waits while the queue is full, allowing normal OS-pipe backpressure to reach Pi. It exits promptly when the supervisor is no longer starting or ready, so shutdown cannot deadlock on an undrained queue. New supervisor tests cover both sustained stdout pressure and shutdown with a full queue.

### 4. The Node bridges did not implement Pi's strict JSONL framing

Both bridge processes used Node `readline`. Pi's RPC contract is LF-only JSONL; Unicode line and paragraph separators are valid inside JSON strings and must not split records.

A shared dependency-free `bridge/jsonl.mjs` now owns framing for stdin and local sockets. Tests cover:

- U+2028 and U+2029 inside JSON strings;
- CRLF input and a final unterminated record;
- oversized record discard followed by successful recovery.

This also removes repeated framing logic from the two bridge processes.

### 5. Orchestration transport flaps were presented as hard state loss

The orchestration bridge now:

- waits 2.5 seconds before surfacing a disconnect, so quick adapter replacement is invisible;
- reconnects with bounded exponential backoff and jitter rather than retrying every 500 ms forever;
- keeps the last valid snapshot visible as stale during a real outage;
- handshakes with a per-adapter `producerId`, allowing generation counters to restart safely;
- retires superseded producers and ignores buffered or reconnecting records from their sockets;
- fails pending actions immediately when their adapter is replaced;
- sends fewer forced snapshots and polls every 5 seconds instead of every 2 seconds.

### 6. Stopped/failed subagents could incorrectly complete tasks

Only the subagent extension's successful terminal states (`completed` and `steered`) now mark a task complete and unlock dependents. Stopped, aborted, and failed runs return the task to `pending`, clear stale owner/agent/result metadata, retain a useful failure note, and clear cascade state. The adapter now follows `pi-tasks` mutation semantics exactly: metadata keys are removed with null tombstones, while the store's empty-string owner clear value is filtered out of snapshots. On session start, persisted `in_progress` tasks are reconciled with the live manager and orphaned runs become retryable; shutdown force-resets remaining work before Pi disposes its in-memory agents. Late duplicate terminal events are ignored once handled. This prevents a failed or interrupted orchestration branch from silently advancing its DAG or appearing to run forever.

## Verification completed in this environment

- `node --test bridge/*.test.mjs` (15/15, including real orchestration-adapter and bridge reconnect lifecycle tests)
- `node --check bridge/pi-bridge.mjs`
- `node --check bridge/orchestration-adapter.mjs`
- `node --check bridge/orchestration-core.mjs`
- `node --check bridge/jsonl.mjs`
- `python3 -m json.tool bridge/protocol.schema.json`
- TypeScript no-emit check of `bridge/protocol.ts`
- Python `tomllib` parse of `Cargo.toml`
- Static delimiter validation of every changed Rust file
- Pi SDK sidecar smoke test with a synthetic 0.82.0 package root
- `git diff --check`

The Rust toolchain was not present in the execution environment, and outbound DNS was unavailable, so Rust compilation and tests could not be run here. Run the following in a normal development environment before merging:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run
cargo build --release
```

For performance validation, compare the same long streaming session in both the new development profile and release mode. Capture frame time, controller notification rate, RPC queue depth, and Pi process restarts. The code changes are designed to remove artificial restarts and redundant invalidations; an exact FPS improvement still needs measurement on the target OS/GPU.

## Reference implementations reviewed

- Zed development profiles and GPUI list/entity ownership: <https://github.com/zed-industries/zed>
- Pi RPC framing, lifecycle, and current subagent example: <https://github.com/earendil-works/pi>
