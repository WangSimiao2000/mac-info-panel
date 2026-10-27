# Info Panel

Info Panel 是使用 Rust 构建的跨平台小屏信息面板，目标平台为 macOS、Windows 和 Linux。

当前处于架构预览阶段。UI 使用 Slint，插件通过 Extism/Wasmtime 沙箱运行。插件只返回语义化 `PageModel`，界面由宿主统一渲染，以保持一致的视觉风格。

## 架构

```text
apps/desktop                    桌面程序
crates/panel-core               状态与导航
crates/panel-model              页面和组件模型
crates/panel-plugin-api         插件协议
crates/panel-plugin-host        插件加载与输出验证
crates/panel-plugin-runtime-extism
crates/panel-ui                 Slint UI
plugins/example-status          真实 WASM 示例插件
themes/default                  Design tokens
```

插件计划支持：

- `wasm@1`：编译后的 WebAssembly 插件
- `lua@1`：在受限 Lua-in-WASM runner 中执行的 Lua 插件

公共插件不支持 C ABI 动态库，也不能提供任意 UI 代码。

## 开发

要求：

- Rust 1.99.1，通过 `rust-toolchain.toml` 固定
- `wasm32-unknown-unknown` target

运行完整检查：

```bash
./Scripts/check-rust.sh
```

构建桌面程序：

```bash
cargo build --release -p info-panel
```

运行架构预览：

```bash
cargo run -p info-panel
```

构建示例插件：

```bash
cargo build \
  --manifest-path plugins/example-status/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
```

## 设计文档

```text
.kiro/specs/rust-cross-platform-rewrite/
├── requirements.md
├── design.md
└── tasks.md
```

## 旧 Swift 版本

最后一个纯 Swift 版本保存在 Git 分支：

```text
legacy/swift
```

新版本不读取或迁移旧版配置。

## License

[MIT](LICENSE)
