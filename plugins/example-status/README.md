# Example Status plugin

最小的 Info Panel WebAssembly 插件，用于验证 `metadata`、`validate_config`、`refresh` 三项版本化 JSON ABI、语义页面模型和 Extism 沙箱加载流程。manifest 使用 `runtime: wasm@1`；插件只获准读取宿主提供的平台标识，未获准读取 locale、配置、网络或缓存。

构建：

```bash
cargo build --manifest-path plugins/example-status/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
```

产物位于：

```text
plugins/example-status/target/wasm32-unknown-unknown/release/info_panel_plugin_example_status.wasm
```

插件没有网络、文件、进程或缓存权限。
