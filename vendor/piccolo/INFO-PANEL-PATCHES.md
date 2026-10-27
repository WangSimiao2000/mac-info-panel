# Piccolo provenance and Info Panel patches

- Upstream project: Piccolo
- Version: 0.3.3
- Upstream commit: `9d4906be78959db2194f1fdc29e0b4eccde81134`
- Source: https://github.com/kyren/piccolo
- License: dual-licensed under MIT or CC0-1.0; both upstream license texts are retained in this directory.

Info Panel carries these local patches:

1. Disable `ahash` runtime entropy and seed `math.random` deterministically so `wasm32-unknown-unknown` builds without WASI or JavaScript entropy imports. Calling `math.randomseed()` without arguments restores the same fixed seed.
2. Add non-negative checked fuel consumption. `Executor::step` reserves its base cost before state mutation, then reserves complete callback, sequence, or single-opcode costs before removing or executing a frame. Underfunded operations remain untouched, and exact-cost terminal operations still report completion.
3. Execute at most one prepaid VM opcode per executor iteration; call/return/frame-changing opcodes are still counted before leaving `run_vm`.
4. Expose read-only thread and executor frame counts so the runner can check its stack ceiling before and after each bounded step.
5. Make the raw-slice reference explicit for Rust 1.99's `dangerous_implicit_autorefs` lint.

Disabling runtime entropy does not promise stable hash-table iteration order across processes or builds. Plugin output is parsed as JSON and must not depend on object-key order.
