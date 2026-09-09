# Terminal snapshot measurement

Measured on Windows with Rust 1.97.0 in the repository's optimized development/test profile, 8 September 2026. Each result is the mean of 64 iterations of the deterministic, explicitly invoked benchmark. These are local timing samples, not a guarantee across hardware or a measurement of PTY throughput, GPU rendering, frame pacing, or application startup.

Command: `cargo test benchmark_terminal_engine_feed_and_snapshot -- --ignored --nocapture`

| Grid | Content | Before, ms/snapshot | After, ms/snapshot | Before text allocations | After text allocations |
|---|---|---:|---:|---:|---:|
| 120 × 40 | Blank | 0.351 | 0.140 | 4,800 | 0 |
| 120 × 40 | Dense ASCII | 0.370 | 0.391 | 4,800 | 0 |
| 240 × 100 | Blank | 2.484 | 1.091 | 24,000 | 0 |
| 240 × 100 | Dense ASCII | 3.114 | 0.658 | 24,000 | 0 |

The measured allocation cost justified changing cell text from `String` to `Cow<'static, str>`. Printable ASCII, spaces, and empty spacer cells now borrow immutable text. Non-ASCII and combining-character cells retain owned text. This introduces no mutable cache, synchronization, or new dependency. The smaller dense sample did not improve; timings should not be generalized to every workload.

Regression tests cover every printable ASCII character, Unicode and combining marks, wide cells, selection, mouse coordinates, and terminal protocol responses. The benchmark remains ignored during normal tests to avoid machine-dependent timing assertions.

The Files and Git panels use GPUI's virtualized uniform lists, and the file editor reuses the rope-backed GPUI editor component. Directory/Git/file I/O runs off the event loop. Project-owned entities retain editor state and terminal workers when another project or tab is visible.
