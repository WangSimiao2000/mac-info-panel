# Prompt 06: Lua-in-WASM plugin runner

Start only after plugin resource limits and host-function permission checks are complete.

Implement tasks 5.12 through 5.14. Build a controlled `lua-runner.wasm` that accepts a Lua source file and versioned JSON request, invokes only the documented plugin entry points, and returns the same `RefreshResponse` used by WASM plugins.

The Lua environment must not expose filesystem, process, environment, dynamic native modules, arbitrary sockets, `dofile`, `loadfile`, unrestricted `require`, or debug APIs that bypass limits. Host HTTP, cache, logging, locale, and platform data remain explicit permission-checked host calls.

Apply instruction/fuel, wall-time, memory, recursion/stack, output-size, and host-call limits. Add fixtures for infinite loops, recursive overflow, allocation bombs, oversized strings/tables, forbidden globals, malformed return values, and undeclared network access.

Create `plugins/example-lua-status` and execute a real end-to-end smoke test through the WASM runner. Do not fall back to native `mlua` merely because the WASM build is difficult; document and escalate blockers instead.

Run all checks and create one focused commit.
