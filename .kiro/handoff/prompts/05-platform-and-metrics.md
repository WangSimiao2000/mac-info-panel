# Prompt 05: Platform traits and generic metrics

Create `panel-platform`, platform-specific crate skeletons, and `panel-metrics` according to the design dependency rules.

First define capability-reporting traits for window, displays, tray, autostart, and media. Shared code must not pretend unsupported platform data exists. Keep OS framework types inside platform crates.

Implement portable CPU, memory, disk, network, uptime, and load data where available. Counter deltas must treat decreasing counters as resets, maintain per-interface baselines, use monotonic elapsed time, and clamp percentages. Hidden panels must be able to reduce or pause sampling.

Prioritize trait design and testable generic sampling logic before macOS-specific behavior. Use fake snapshots and clocks for unit tests. Do not copy Swift implementation into `main`; consult `legacy/swift` only to understand expected output.

Run full validation and create focused commits separated into traits and metrics if the diff becomes large.
