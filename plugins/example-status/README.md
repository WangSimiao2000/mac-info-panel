# Example Status plugin

最小的 Mac Info Panel WebAssembly 插件，用于验证版本化请求、语义页面模型和 Extism 沙箱加载流程。

构建：

```bash
cargo build --manifest-path plugins/example-status/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
```

产物位于：

```text
plugins/example-status/target/wasm32-unknown-unknown/release/mac_info_panel_plugin_example_status.wasm
```

插件没有网络、文件、进程或缓存权限。
