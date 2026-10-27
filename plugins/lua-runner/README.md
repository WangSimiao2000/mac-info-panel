# Info Panel Lua runner

`lua-runner.wasm` embeds Piccolo 0.3.3, a pure-Rust, stackless Lua VM. Piccolo was selected instead of `mlua` because `mlua`'s vendored PUC-Rio Lua build does not support `wasm32-unknown-unknown`; no native Lua VM or native fallback is used.

The checked-in Piccolo source is pinned under `vendor/piccolo`. `vendor/piccolo/INFO-PANEL-PATCHES.md` records upstream commit `9d4906be78959db2194f1fdc29e0b4eccde81134`, local patches, and dual-license provenance. Entropy imports are disabled and `math.random` starts from a deterministic seed, but hash-table iteration order is not promised across processes or builds.

The runner creates a fresh VM per ABI call from `Lua::empty()`. It allowlists safe base, math, string, and table operations, and removes coroutine, metatable/raw access, collection controls, `table.unpack`, `debug`, `dofile`, `host`, `io`, `load`, `loadfile`, `os`, `package`, and `require`. It exports only `metadata`, `validate_config`, and `refresh`. Extism creates the runner with all hosts disallowed, WASI disabled, built-in HTTP blocked, and log messages discarded before reading.

Limits are enforced at execution time:

- `maxInstructions` is a conservative Piccolo fuel budget. Each executor step reserves 4 units before state mutation; callbacks cost 8 additional units, sequences cost 4 additional units, and a Lua VM opcode costs 1 additional unit. The runner supplies slices of at most 12 units without exceeding the total remaining budget.
- Checked consumption never creates fuel debt. An underfunded callback, sequence, or opcode remains unexecuted, and an unfinished zero-progress step fails immediately.
- Source initialization and the selected entry point share one budget. Linear work inside a callback or individual standard-library operation is not claimed as per-element fuel accounting; wall time and the outer WASM memory ceiling remain the hard fallback boundaries.
- Extism/Wasmtime enforces wall time and the only hard pre-allocation memory boundary, the outer WASM page ceiling.
- Piccolo allocation metrics provide a secondary detector between bounded fuel steps. `max_lua_memory_bytes` must not exceed the outer WASM limit and is not described as an allocator hard limit.
- Piccolo frame depth is checked before and after every bounded step, including completion.
- Lua output is counted through a no-allocation writer before allocating the exact-size final buffer. JSON escaping, table nodes, array continuity, maximum index, and encoded bytes are bounded during traversal.
- Source bytes are checked before allocation. The complete source/request/limits wrapper is serialized through a stopping writer bound to the host input limit.
- Host-call budget is fixed at zero until permission-checked host functions are implemented. Any non-zero budget and every attempted `host.*` call fail closed.

Build:

```bash
cargo build --manifest-path plugins/lua-runner/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
```
