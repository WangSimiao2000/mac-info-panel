# Info Panel Agent Instructions

## Product scope

Info Panel is a Rust-based cross-platform secondary-display dashboard for macOS, Windows, and Linux.

Non-negotiable decisions:

- Product name: **Info Panel**. Do not reintroduce “Mac” into new package, protocol, plugin, or UI names.
- `main` is Rust-only. The last pure Swift implementation is preserved on `legacy/swift` for visual and behavioral reference only.
- Do not read, migrate, modify, or depend on old Mac Info Panel configuration.
- UI stack: Slint. Do not add egui, raylib, RmlUi, HTML, or JavaScript UI without a new explicit decision.
- Plugins return semantic `PageModel` values. The host owns all rendering and design tokens.
- Public plugins run inside a WebAssembly security boundary. Native C ABI plugins are not accepted.
- Lua plugins must execute through a restricted Lua-in-WASM runner and receive the same limits as WASM plugins.
- MC Ops belongs in a separate plugin repository. Do not restore its old server/client implementation to this repository.

## Read before changing code

1. `HANDOFF.md`
2. `.kiro/specs/rust-cross-platform-rewrite/requirements.md`
3. `.kiro/specs/rust-cross-platform-rewrite/design.md`
4. `.kiro/specs/rust-cross-platform-rewrite/tasks.md`
5. The crate being changed and its direct dependents

Treat the spec task list as the source of truth. Mark a task complete only after its stated behavior is implemented and verified.

## Architecture boundaries

- `panel-model`: serializable domain and page types; no UI, networking, platform, or runtime dependencies.
- `panel-config`: Info Panel schema and validation; no old-config compatibility.
- `panel-core`: deterministic state transitions; no Slint, network, filesystem, or wall-clock calls.
- `panel-plugin-api`: versioned manifest and request/response protocol; no Extism types.
- `panel-plugin-runtime-extism`: Extism adapter only.
- `panel-plugin-host`: permissions, lifecycle, ABI calls, output validation, and failure isolation.
- `panel-ui`: Slint renderer; no direct network or plugin runtime access.
- Platform crates implement traits and keep OS APIs out of shared crates.

Dependencies should point inward toward model/protocol crates. Do not make lower-level crates depend on UI or desktop applications.

## Security requirements

- Validate all plugin output after deserialization.
- Default plugin capabilities to none.
- Route network, cache, logging, locale, and platform data through explicit host functions.
- Do not expose filesystem, process, environment, dynamic module, or arbitrary socket access to plugins.
- Apply execution-time, memory, output-size, component-count, text-length, and request-rate limits.
- Parse URLs structurally. Do not use string prefixes for security checks.
- Reject URL credentials. Public HTTP is forbidden; HTTP is only permitted for verified loopback addresses.
- Do not log secrets, complete plugin configuration, or lyrics query contents.

## Development workflow

Use the pinned toolchain in `rust-toolchain.toml`. Cargo downloads use the project-local RSProxy configuration.

Before each commit run:

```bash
./Scripts/check-rust.sh
git diff --check
```

`check-rust.sh` runs formatting, workspace checks, tests, strict Clippy, builds the example WASM plugin, and performs a real Extism smoke test.

Keep commits focused. Update `tasks.md` in the same commit as an implemented task. Do not mark partial work complete. Do not push, create remote repositories, publish packages, sign releases, or rename the GitHub repository without explicit user approval.

## Git state

- Primary branch: `main`
- Swift archive branch: `legacy/swift`
- Do not merge `legacy/swift` back into `main`.
- Preserve user changes and avoid destructive Git commands.
