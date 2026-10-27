# Info Panel Rust 跨平台重构任务

状态标记：`[ ]` 未开始，`[-]` 进行中，`[x]` 完成。

## 0. 决策与基准

- [x] 0.1 选择 Slint + Rust UI 路线。
- [x] 0.2 确定 WASM 沙箱插件，不使用原生动态库。
- [x] 0.3 确定宿主统一渲染语义页面模型。
- [ ] 0.4 记录当前 Swift 版本各页面基准截图和窗口尺寸。
- [ ] 0.5 建立当前功能行为清单和平台差异表。
- [x] 0.6 确认 Rust MSRV、Slint 和 Extism 精确版本并锁定依赖。

## 1. Workspace 骨架

- [x] 1.1 安装并确认 Rust 工具链。
- [x] 1.2 创建根 Cargo workspace 和 `rust-toolchain.toml`。
- [-] 1.3 创建核心、模型、配置、UI、插件和平台 crates。
- [-] 1.4 创建 desktop 与 plugin-cli 应用。
- [-] 1.5 配置 fmt、clippy、test、deny 和 audit。
- [x] 1.6 添加 macOS、Windows、Linux CI 构建矩阵。
- [x] 1.7 将纯 Swift 实现固定保存在 `legacy/swift`，并从 `main` 移除。

## 2. 共享模型和核心状态

- [x] 2.1 实现 `PageId`、`PluginId` 和版本类型。
- [x] 2.2 实现 `PageModel`、Section 和首批 Widget 类型。
- [x] 2.3 实现页面模型边界验证。
- [x] 2.4 实现 `AppState`、`AppEvent` 和 reducer。
- [x] 2.5 实现页面导航、自动轮播和隐藏状态调度策略。
- [-] 2.6 使用 fake clock 测试取消、generation 和轮播。

## 3. 配置与版本升级

- [x] 3.1 实现安全的 JSONC 注释解析。
- [x] 3.2 实现 Info Panel schemaVersion 1 强类型配置。
- [x] 3.3 实现跨字段验证和字段级诊断。
- [-] 3.4 验证 `initialPage`、唯一 ID、页面引用、URL 和范围。
- [ ] 3.5 为未来 Info Panel schema 升级建立纯函数迁移框架。
- [ ] 3.6 实现备份、临时写入和原子替换。
- [-] 3.7 测试字符串中的注释符、转义、错误路径和升级失败。
- [ ] 3.8 确保禁用/未安装歌词时配置、页面和任务一致。
- [x] 3.9 从平台专属新路径原子引导配置，并用字段级错误初始化核心状态。

## 4. HTTP 安全客户端

- [x] 4.1 实现 HTTPS 默认和 loopback HTTP 例外。
- [x] 4.2 拒绝 URL 凭据和不允许的 scheme。
- [x] 4.3 实现连接、请求和资源超时。
- [x] 4.4 实现流式响应大小限制。
- [x] 4.5 默认禁用 Cookie 和重定向。
- [x] 4.6 实现逐跳重定向复验的可选策略。
- [x] 4.7 防御 DNS 重绑定和公开域名到私网解析。
- [x] 4.8 实现域名、并发和速率限制。
- [x] 4.9 为 URL、响应边界和重定向添加测试。

## 5. 插件 API 与运行时

- [x] 5.1 定义版本化 plugin manifest schema。
- [x] 5.2 定义 `metadata`、`validate_config` 和 `refresh` ABI。
- [x] 5.3 定义 `PluginRuntime` trait。
- [x] 5.4 实现 Extism runtime adapter。
- [x] 5.5 实现执行时间、内存和输出大小限制。
- [ ] 5.6 实现宿主 HTTP、缓存、日志、locale 和 platform functions。
- [x] 5.7 实现插件权限授权和权限升级检测。
- [x] 5.8 实现插件隔离状态、退避、抖动和手动重试。
- [x] 5.9 添加无限循环、OOM、超大输出和越权插件夹具。
- [x] 5.10 建立 API 兼容测试并记录弃用策略。
- [x] 5.11 定义 `runtime: wasm@1 | lua@1` manifest 字段。
- [x] 5.12 构建受限 `lua-runner.wasm`，禁用 OS、文件、进程和动态模块能力。
- [x] 5.13 添加 Lua 指令、内存、输出和宿主调用限额测试。
- [x] 5.14 创建 example-lua-status 插件并通过真实 runner smoke test。

## 6. 插件包和 CLI

- [ ] 6.1 定义确定性 `.info-panel-plugin` 包格式。
- [ ] 6.2 实现包结构、路径穿越和大小校验。
- [ ] 6.3 实现 SHA-256 和签名验证。
- [ ] 6.4 实现 `plugin-cli new`。
- [ ] 6.5 实现 `plugin-cli validate`。
- [ ] 6.6 实现 `plugin-cli test`。
- [ ] 6.7 实现 `plugin-cli pack`。
- [ ] 6.8 实现本地开发模式和未签名警告。

## 7. Slint UI 和设计系统

- [-] 7.1 从当前 UI 提取 design tokens。
- [x] 7.2 创建 token 到 Rust/Slint 的生成流程。
- [x] 7.3 实现面板根布局和自适应 breakpoints。
- [x] 7.4 实现 Metric、Status、Gauge、Text、Progress、List、Chart。
- [x] 7.5 实现统一插件页面 renderer。
- [ ] 7.6 实现总览页。
- [ ] 7.7 实现本机状态页。
- [ ] 7.8 实现歌词页和唱片动画。
- [ ] 7.9 实现页面转场和 Spring 风格动画。
- [-] 7.10 实现 Reduce Motion、文字缩放和高对比度。
- [x] 7.11 实现小屏滚动、字段上限和页面指示器折叠。
- [ ] 7.12 建立视觉截图测试。

## 8. 指标、媒体与歌词

- [x] 8.1 定义系统指标 provider trait。
- [x] 8.2 实现跨平台通用指标与平台特有能力标记。
- [x] 8.3 安全处理 counter 回绕、重置和接口变化。
- [-] 8.4 定义媒体 provider trait 和统一事件。
- [ ] 8.5 实现稳定 `TrackIdentity`。
- [ ] 8.6 实现 LRC 多时间戳、翻译和容错 parser。
- [ ] 8.7 实现可注入歌词 provider。
- [ ] 8.8 实现有界 LRU、TTL 和过期清理。
- [ ] 8.9 实现第三方歌词隐私提示与日志脱敏。
- [ ] 8.10 添加纯逻辑和并发竞态测试。

## 9. macOS 平台对齐

- [ ] 9.1 实现显示器枚举、选择和热插拔。
- [ ] 9.2 实现 nonactivating panel 和全 Space 行为。
- [ ] 9.3 实现可见区域、菜单栏和 Dock 避让。
- [ ] 9.4 实现托盘菜单和页面控制。
- [ ] 9.5 实现系统材质/模糊适配。
- [ ] 9.6 实现 Now Playing provider 和可选回退。
- [ ] 9.7 实现登录启动。
- [ ] 9.8 对比 Swift 版本功能与视觉基准。
- [ ] 9.9 构建、签名并验证 preview `.app`。

## 10. 内置功能插件化

- [x] 10.1 创建 example-status WASM 插件。
- [ ] 10.2 将 HTTP JSON 能力实现为受限通用插件或官方插件。
- [ ] 10.3 设计并实现 MC Ops 插件。
- [ ] 10.4 保留并加固服务端 collector 的响应体和包长度限制。
- [ ] 10.5 将 MC Ops 移到独立插件仓库。
- [ ] 10.6 为 MC Ops 建立测试、版本和 release。

## 11. 插件索引仓库

- [ ] 11.1 创建独立索引仓库和 schema。
- [ ] 11.2 实现索引签名和客户端验证。
- [ ] 11.3 实现插件发现、安装、升级、禁用和卸载。
- [ ] 11.4 实现权限变更确认。
- [ ] 11.5 CI 验证插件哈希、签名、许可证和 API 兼容性。
- [ ] 11.6 发布 example-status 和 MC Ops 首个版本。

## 12. Windows 平台

- [ ] 12.1 实现窗口、显示器和 DPI 适配。
- [ ] 12.2 实现托盘。
- [ ] 12.3 实现 GSMTC 媒体 provider。
- [ ] 12.4 实现 Mica/Acrylic 能力适配。
- [ ] 12.5 实现开机启动和安装包。
- [ ] 12.6 完成功能、视觉和长稳测试。

## 13. Linux 平台

- [ ] 13.1 实现 Wayland 优先、X11 兼容窗口路径。
- [ ] 13.2 实现托盘/StatusNotifierItem。
- [ ] 13.3 实现 MPRIS 媒体 provider。
- [ ] 13.4 记录 compositor 材质和窗口能力差异。
- [ ] 13.5 实现 XDG autostart 和安装包。
- [ ] 13.6 完成功能、视觉和长稳测试。

## 14. 文档、发布与替代

- [ ] 14.1 更新 README 的精确工具链和平台依赖。
- [ ] 14.2 记录配置、日志、插件权限和歌词隐私。
- [ ] 14.3 建立版本、tag、changelog 和 release workflow。
- [ ] 14.4 配置 macOS Developer ID、公证和产物验证。
- [ ] 14.5 配置 Windows 签名和 Linux 产物验证。
- [ ] 14.6 运行加速七天长稳测试。
- [ ] 14.7 提供版本升级失败时的回滚说明。
- [x] 14.8 将纯 Swift 实现归档到 `legacy/swift` 分支。
