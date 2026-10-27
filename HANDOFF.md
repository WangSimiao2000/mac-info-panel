# Info Panel Engineering Handoff

## Snapshot

Handoff date: 2026-10-28

Repository path:

```text
/Volumes/exSSD/Projects/mac-info-panel
```

The directory and current GitHub remote still use the old repository name. The product, Cargo package, UI title, configuration, and plugin format are named **Info Panel**. Renaming the local directory or remote repository requires a separate user-approved operation.

Branches:

- `main`: Rust-only Info Panel implementation
- `legacy/swift`: final pure Swift version at `be0789a`

At handoff, `main` is seventeen commits ahead of `origin/main`; neither `main` nor `legacy/swift` has been pushed by this agent.

Recent migration commits:

```text
6c4bb1b 补强页面布局参数校验
fe8ebd2 实现 PageModel 视图转换
4d7f69d 更新平台与指标交接状态
59f4f91 集成平台与指标工作区
a4a10a5 实现通用指标采样逻辑
25a3dad 定义平台能力与服务契约
94b6c03 更新配置启动交接状态
051a661 连接配置引导与桌面启动
39ef884 更新 HTTP 客户端交接状态
f02e3c0 实现安全宿主 HTTP 客户端
3817e79 添加 KiroCrew 工程交接包
c6e7120 实现统一应用状态与事件归约
9a2d056 实现 Info Panel 独立配置格式
5d62cac 将 Swift 版本归档到遗留分支
fe9cc2c 将跨平台版本命名为 Info Panel
212b1c4 建立 Rust 跨平台插件架构骨架
```

## Implemented

- Rust 1.99.1 pinned with rustfmt and Clippy.
- `wasm32-unknown-unknown` development target.
- Cargo workspace and macOS/Windows/Linux CI check matrix.
- Slint 1.18.1 static architecture-preview window.
- Design token seed file at `themes/default/tokens.json`.
- Versioned page model with Metric, Status, Gauge, Text, Progress, List, and Chart widgets.
- Page-model limits and untrusted identifier/numeric validation.
- Extism 1.41.0 runtime adapter with checked execution timeout, exact per-call cancellation tokens, real WASM page memory ceiling, pre-copy output ceiling, and a monotonic pre-read `error_set` boundary.
- Host ABI input uses a stopping limited writer; raw plugin configuration is byte-limited before JSON allocation.
- Runtime panics are caught without formatting their payload, invalidate the instance, and require reload.
- `runtime: wasm@1 | lua@1` manifest discriminator with strict entry matching; `lua@1` fails closed until the Lua-in-WASM runner exists.
- Complete `metadata`, `validate_config`, and `refresh` JSON ABI with package/manifest/metadata identity checks.
- Host/API/minimum-version compatibility, permission grants, permission-upgrade diffs, and host-mediated `panel-http` network permissions.
- Per-plugin generation checks, exact per-call cancellation, bounded diagnostics, exponential backoff, real production jitter with injectable deterministic sources, and one manual retry per backoff cycle.
- Malicious runtime fixtures for infinite loops, memory growth, oversized output/error, monotonic error latching, malformed JSON, invalid UTF-8, cancellation barriers at every boundary, stale generations, permission escape, direct Extism HTTP, runtime panic, and all Metadata/ValidateConfig/Refresh compatibility directions.
- Plugin manifest, request, response, host loading, and post-deserialization PageModel validation.
- Structural network-origin validation, including rejection of credentials and fake localhost domains.
- Secure host HTTP client with exact origin permissions and injected IO.
- HTTPS-first policy with resolved loopback checks for plain HTTP.
- DNS rebinding defense, pinned addresses, and redirect hop revalidation.
- Separate connect, request, and whole-resource timeouts.
- Bounded streamed responses, redirect hops, concurrency, and per-plugin request rates.
- Cookies and redirects disabled by default; typed errors omit sensitive request data.
- Real Rust-to-WASM example plugin and real Extism invocation test.
- Independent Info Panel JSONC schemaVersion 1 configuration.
- Platform-specific new configuration paths for macOS, Windows, and Linux.
- Locked first-run creation using same-directory atomic writes.
- Invalid existing configuration is reported and never replaced.
- Validated configuration initializes `AppState` with explicit built-in page IDs.
- JSONC parser that preserves comment markers inside strings.
- Configuration validation for IDs, initial page, bounds, duplicates, and unknown fields.
- Deterministic `AppState`/`AppEvent` reducer.
- Capability-reporting traits for window, displays, tray, autostart, and media.
- Explicitly unsupported macOS, Windows, and Linux adapter skeletons.
- Generic metrics sampling with injected source and monotonic clock.
- Safe CPU and network deltas, per-interface baselines, and reset handling.
- Visibility-aware sampling policy and bounded metric normalization.
- Automatic page rotation, visibility behavior, plugin generation race protection, bounded diagnostics.
- PageModel-to-view conversion for every current widget variant.
- Adaptive columns, bounded text and lists, chart normalization, and indicator folding.
- Strict finite layout input validation and Reduce Motion propagation.
- Pure Swift implementation removed from `main` and retained on `legacy/swift`.

## Verified baseline

The last full local validation passed:

```bash
./Scripts/check-rust.sh
```

It covered 112 normal tests plus one real Extism/WASM smoke test, workspace check, formatting, strict Clippy, and release plugin compilation.

Release desktop build also passed previously:

```bash
cargo build --release -p info-panel
```

## Important limitations

- The Slint window is static; `PageView` conversion is not bound to components yet.
- Host HTTP, cache, logging, locale, and platform functions are not yet exposed to plugins; Extism built-in HTTP and WASI remain disabled.
- Lua-in-WASM runner does not exist, so `lua@1` manifests are intentionally rejected.
- No plugin package parser, signature verification, plugin CLI, or repository client exists.
- Platform traits and adapter skeletons exist, but native OS behavior does not.
- No real metrics, media, lyrics, tray, display-selection, autostart, or installer implementation exists.
- Cross-platform GitHub Actions were added but have not been observed running remotely.
- Slint licensing must be reviewed before public binary distribution. Do not assume MIT alone covers the final linked application.

## Decisions that must not drift silently

- No old configuration compatibility.
- No native public plugins.
- No arbitrary plugin UI.
- Lua remains behind WASM isolation.
- Host performs all privileged IO.
- The plugin store is a separate repository.
- MC Ops becomes a separate plugin and repository.

If a constraint must change, update requirements, design, tasks, and this file in the same decision commit.

## Recommended execution order

1. Finish plugin manifest runtime selection and hard resource limits.
2. Connect `PageView` to generated tokens and Slint components.
3. Implement native platform services and metrics sources.
4. Prototype Lua-in-WASM runner after the host limits are complete.
5. Implement plugin packaging and `plugin-cli`.
6. Create MC Ops as a separate repository only after user approval for remote repository creation.

Prompts for these work items are under `.kiro/handoff/prompts/`.

## Validation and commit policy

For each work item:

1. Read the applicable spec sections.
2. Inspect existing code before editing.
3. Implement only the scoped task.
4. Add focused tests for new trust boundaries and pure logic.
5. Run `./Scripts/check-rust.sh` and `git diff --check`.
6. Update `tasks.md` conservatively.
7. Create one focused commit.
8. Report anything not verifiable on the local macOS machine.
