# Prompt 02: Plugin protocol and runtime hardening

Finish the currently partial plugin tasks 5.1, 5.2, 5.5, 5.7, 5.8, 5.9, 5.10, and 5.11 after `panel-http` exists.

Add a manifest runtime discriminator:

```text
wasm@1
lua@1
```

Complete metadata, configuration validation, and refresh calls without exposing Extism types from `panel-plugin-api`. Enforce host-version/API compatibility, package identity, granted permissions, execution timeout, memory ceiling, output-byte ceiling, PageModel limits, and bounded diagnostics.

Implement per-plugin lifecycle state with cancellation, generation checks, exponential backoff, jitter from an injectable deterministic source, and one explicit manual retry. Plugin failure must never terminate the desktop host.

Add malicious fixtures for infinite loops, memory growth, oversized output, malformed JSON, invalid UTF-8, stale generations, undeclared HTTP origins, and incompatible API versions. Keep network access host-mediated through `panel-http`.

Do not implement native C ABI plugins. Do not execute Lua in a native in-process VM. Update specs if an Extism API limitation forces a design change. Run the complete validation and create one focused commit.
