# Info Panel Rust 跨平台重构设计

## 1. 架构决策

### 1.1 UI

采用 Slint 构建宿主 UI。页面只渲染宿主拥有的语义化组件。插件不能注入 Slint、HTML 或脚本。

这样可以保持统一视觉、进行模型验证和视觉测试，也能避免第三方页面获得 UI 线程和宿主 API 的直接访问权。

### 1.2 插件运行时

首版使用 Extism 承载 WebAssembly 插件。业务代码只依赖 `PluginRuntime` trait，Extism 封装位于单独 crate，避免协议与运行时绑定。

插件通信使用版本化 serde 数据模型。首版使用 JSON 作为 ABI payload，优先保证可调试性和多语言插件开发体验；后续可在 API 大版本中增加 MessagePack 或 Component Model adapter。

Lua 插件不直接嵌入宿主原生 Lua VM。宿主加载受控的 `lua-runner.wasm`，再将 `plugin.lua`、配置和调用上下文传入 runner。Lua 和原生 WASM 插件共享同一权限检查、超时、内存限制和 `PageModel` 验证流程。公共插件仓库不接受 C ABI 动态库。

运行时标识是 manifest 的必填字段：`wasm@1` 只能对应 `plugin.wasm`，`lua@1` 只能对应 `plugin.lua`。在受限 `lua-runner.wasm` 可用前，宿主必须拒绝 `lua@1`，不得回退到原生 Lua VM。Extism 的内建 HTTP 和 WASI 均关闭；网络能力只接入 `panel-http` 校验后的宿主函数。

插件 ABI 1 包含三个 JSON 调用：`metadata(request) -> response`、`validate_config(request) -> response` 和 `refresh(request) -> response`。宿主在加载时比较 package identity、manifest 和 metadata，在刷新前验证配置，并在反序列化后验证 `PageModel`。每次调用都受同一执行时间、WASM page、输入、输出和错误字节上限约束。适配器在 Extism 内建 `error_set`、`http_request` 和日志函数读取之前以同名 host functions 覆盖入口。`error_set` 只查询 allocation 长度而不读取或记录插件错误正文；直接 Extism HTTP 固定拒绝，日志正文直接丢弃。超限错误、无效指针和未中介网络均 fail closed。宿主捕获 runtime unwind，丢弃 panic payload，并永久标记该实例失效，后续调用必须重载。

协议层限制解码后的 manifest 名称、版本、source、license、页面和网络 origin 的数量与单项字节数。任务 6.2 的包解析器仍必须在 JSON 反序列化前限制原始 `manifest.json` 文件字节数；协议字段限制不能替代原始文件边界。

插件 API 兼容规则为同一 major 内，插件 minor 小于或等于宿主 minor；不同 major 或未来 minor 在 `metadata` 后、其他 ABI 调用前拒绝。响应模型在 minor 版本中只能新增由新宿主以 `serde(default)` 接收的可选字段，使新宿主可以读取旧插件响应。请求模型使用 `deny_unknown_fields`，所以一个 major 内的字段集合固定：新宿主只能开始填充 v1.0 已声明的可选字段，不能向旧插件发送新增字段；新增请求字段必须升级 major。所有方向的未知字段均拒绝。废弃字段至少保留一个宿主 minor 版本，并为 Metadata、ValidateConfig 和 Refresh 的当前版本、旧 minor、未来 minor、不同 major、未知字段以及宿主实际拒绝路径保留 JSON 夹具。删除字段或改变语义必须发布新 major。

### 1.3 异步运行时

核心服务使用 Tokio。Slint UI 线程不执行阻塞 IO。后台任务通过有界 channel 将不可变快照发送给 UI 协调器。

### 1.4 重构策略

`main` 只保留 Info Panel Rust 架构。最后一个纯 Swift 实现固定保存在 `legacy/swift` 分支，仅用于行为和视觉参考。Rust 版本不读取旧配置、不复用旧安装状态，也不调用 Swift 二进制。

## 2. 目标目录

```text
.
├── Cargo.toml
├── rust-toolchain.toml
├── apps/
│   ├── desktop/
│   └── plugin-cli/
├── crates/
│   ├── panel-core/
│   ├── panel-model/
│   ├── panel-config/
│   ├── panel-ui/
│   ├── panel-plugin-api/
│   ├── panel-plugin-host/
│   ├── panel-plugin-runtime-extism/
│   ├── panel-http/
│   ├── panel-metrics/
│   ├── panel-media/
│   ├── panel-lyrics/
│   ├── panel-platform/
│   ├── panel-platform-macos/
│   ├── panel-platform-windows/
│   └── panel-platform-linux/
├── plugins/
│   └── example-status/
├── themes/
│   └── default/
├── tests/
│   ├── fixtures/
│   ├── plugin-compat/
│   └── visual/
```

Swift 文件不保留在 `main`；需要参考时从 `legacy/swift` 读取。

## 3. 依赖方向

```text
apps/desktop
  -> panel-core
  -> panel-ui
  -> platform implementation

panel-core
  -> panel-model
  -> panel-config
  -> panel-plugin-host
  -> panel-metrics
  -> panel-media
  -> panel-lyrics
  -> panel-platform

panel-plugin-host
  -> panel-plugin-api
  -> panel-plugin-runtime-extism
  -> panel-http

platform implementations
  -> panel-platform
```

底层 crate 不得依赖 UI。平台 crate 不得依赖具体页面。插件 API 不得依赖 Extism 类型。

## 4. 核心状态

```rust
pub struct AppState {
    pub pages: Vec<PageDescriptor>,
    pub selected_page: PageId,
    pub system: SystemSnapshot,
    pub playback: PlaybackSnapshot,
    pub lyrics: LyricsSnapshot,
    pub plugins: PluginStates,
    pub diagnostics: Vec<Diagnostic>,
}
```

`panel-core` 通过 reducer 接收事件：

```rust
pub enum AppEvent {
    Tick,
    SelectPage(PageId),
    NextPage,
    PreviousPage,
    SystemSnapshotReceived(SystemSnapshot),
    PlaybackChanged(PlaybackSnapshot),
    LyricsResolved(LyricsResult),
    PluginRefreshed(PluginId, PluginResult),
    DisplaysChanged(Vec<DisplayInfo>),
    ConfigurationReloaded(ConfigResult),
}
```

所有异步任务携带 generation 或 cancellation token。旧 generation 的结果不得覆盖新状态。

## 5. 页面模型

```rust
pub struct PageModel {
    pub schema_version: u32,
    pub title: String,
    pub icon: Option<IconRef>,
    pub sections: Vec<Section>,
    pub updated_at: Option<DateTime<Utc>>,
}

pub enum Widget {
    Metric(MetricWidget),
    Status(StatusWidget),
    Gauge(GaugeWidget),
    Text(TextWidget),
    Progress(ProgressWidget),
    List(ListWidget),
    Chart(ChartWidget),
}
```

宿主验证：

- 标题和文本长度
- section/widget 数量
- 数值有限性与范围
- 图表点数
- icon 类型和资源路径
- URL 不得出现在可执行动作中
- 未知 schema 版本

Renderer 根据可用尺寸决定列数、分页和滚动，不由插件提供绝对坐标。

## 6. 插件生命周期

```text
发现包
 -> 校验包路径和结构
 -> 校验 manifest schema
 -> 校验哈希/签名
 -> 比较 API 与宿主版本
 -> 计算权限差异
 -> 用户确认新增权限
 -> 创建受限 runtime
 -> metadata()
 -> validate_config()
 -> schedule refresh()
 -> 验证 PageModel
 -> 发布状态快照
```

失败状态分为：

- package-invalid
- signature-invalid
- incompatible
- permission-denied
- config-invalid
- timeout
- out-of-memory
- invalid-output
- network-error
- runtime-error

插件失败采用指数退避并加入抖动。手动刷新可绕过当前退避一次，但仍受全局速率限制。

## 7. 插件权限和宿主调用

首版 host functions：

```text
host.http_request(request)
host.cache_get(key)
host.cache_put(key, value, ttl)
host.log(level, message)
host.locale()
host.platform()
```

约束：

- `http_request` 只接受 manifest 中声明的 scheme/host/port。
- DNS 解析后拒绝从公开域名重绑定到本地、链路本地或私网地址，除非 manifest 显式声明对应地址范围。
- 重定向逐跳重新校验，首版默认禁止。
- cache key 带插件 ID 命名空间。
- 日志字段执行长度限制和敏感字段过滤。
- 插件时钟和随机数由宿主提供，以便测试重放。

## 8. 插件包与索引

`.info-panel-plugin` 使用确定性 ZIP。manifest 示例：

```json
{
  "manifestVersion": 1,
  "id": "io.github.wangsimiao.mcops",
  "name": "MC Ops",
  "version": "0.1.0",
  "apiVersion": "1.0",
  "minimumHostVersion": "0.2.0",
  "entry": "plugin.wasm",
  "pages": [{ "id": "status", "title": "Minecraft", "icon": "gamepad" }],
  "permissions": {
    "network": ["https://status.example.com"]
  },
  "source": "https://github.com/example/mac-info-panel-plugin-mcops",
  "license": "MIT"
}
```

索引不直接信任插件 manifest；索引重复记录审核后的哈希、权限和版本约束。客户端同时验证索引签名、包哈希和包内 manifest 一致性。

## 9. 配置设计

核心配置支持 JSONC，并从 Info Panel 自己的 `schemaVersion: 1` 开始。loader 分三步：

1. 去除 JSONC 注释并解析原始值。
2. 对未来 schema 版本升级执行纯函数迁移。
3. 反序列化为强类型并执行跨字段验证。

后续版本迁移写入采用临时文件、fsync 和原子替换；替换前保留带时间戳备份。

插件配置存放：

```text
macOS:  ~/Library/Application Support/Info Panel/plugins/<id>/
Windows: %APPDATA%/Info Panel/plugins/<id>/
Linux:  $XDG_CONFIG_HOME/info-panel/plugins/<id>/
```

旧 `~/.config/mac-info-panel` 目录不读取、不修改。

## 10. 平台层

```rust
pub trait PlatformServices: Send + Sync {
    fn window(&self) -> Arc<dyn WindowService>;
    fn displays(&self) -> Arc<dyn DisplayService>;
    fn tray(&self) -> Arc<dyn TrayService>;
    fn autostart(&self) -> Arc<dyn AutostartService>;
    fn media(&self) -> Arc<dyn MediaService>;
}
```

平台能力通过 `Capabilities` 暴露，不伪造不存在的数据：

```rust
pub struct Capabilities {
    pub thermal_state: bool,
    pub non_activating_panel: bool,
    pub background_material: BackgroundMaterialSupport,
    pub global_media: bool,
}
```

## 11. 系统指标

采样器保存原始计数器和采样时间：

```rust
fn checked_counter_delta(current: u64, previous: u64) -> Option<u64> {
    current.checked_sub(previous)
}
```

计数器下降时视为重置，不使用 wrapping subtraction。CPU 最终值钳制至 `0.0..=100.0`。网络按稳定接口 ID 维护各自基线，接口出现或消失不会污染其他接口的速率。

## 12. 歌词与媒体

`TrackIdentity` 优先级：

1. 平台 content ID + player ID
2. player ID + title + artist + album + duration bucket
3. title + artist + duration bucket

缓存采用有界 LRU：

- 成功结果默认 TTL 7 天
- 失败结果默认 TTL 60 秒
- 默认最大 256 首
- 每次写入和定时维护时清理过期项

provider 通过 trait 注入，测试不访问真实网易云接口。第三方请求必须经过与插件 HTTP 相同的安全客户端和隐私日志策略。

## 13. UI 设计系统

`themes/default/tokens.json` 是跨平台设计来源，构建时生成 Rust/Slint 可用常量。

Token 分类：

- color
- typography
- spacing
- radius
- elevation
- motion
- breakpoints

平台只覆盖 material adapter 和系统字体 fallback。插件不能读取或覆盖原始 token，只能使用宿主组件的语义 variant。

## 14. 测试策略

- 单元测试：配置、JSONC、迁移、URL、JSON Pointer、LRC、身份、缓存、counter delta、reducer。
- 属性测试：任意配置/插件输出不得 panic；页面模型尺寸限制保持成立。
- 插件恶意夹具：无限循环、内存增长、超大输出、非法 UTF-8、越权 HTTP、重定向和 DNS 重绑定。
- 集成测试：fake platform、fake clock、fake HTTP、插件调度和取消。
- 视觉测试：固定字体和尺寸生成截图，按平台保存基准并设置批准容差。
- 长稳测试：加速时钟模拟七天轮询，检查任务和缓存上界。

## 15. 发布策略

发布通道：

- preview：验证核心模型、插件边界和平台骨架。
- beta：完成 macOS 功能对齐后提供安装包。
- stable：三平台安装器、升级和回滚验证通过后发布。

正式 macOS 产物使用 Developer ID 和公证；Windows 使用代码签名；Linux 至少提供 AppImage/Flatpak 中经验证的一种，并记录托盘和 Wayland 依赖。
