# Mac Info Panel Rust 跨平台重构设计

## 1. 架构决策

### 1.1 UI

采用 Slint 构建宿主 UI。页面只渲染宿主拥有的语义化组件。插件不能注入 Slint、HTML 或脚本。

这样可以保持统一视觉、进行模型验证和视觉测试，也能避免第三方页面获得 UI 线程和宿主 API 的直接访问权。

### 1.2 插件运行时

首版使用 Extism 承载 WebAssembly 插件。业务代码只依赖 `PluginRuntime` trait，Extism 封装位于单独 crate，避免协议与运行时绑定。

插件通信使用版本化 serde 数据模型。首版使用 JSON 作为 ABI payload，优先保证可调试性和多语言插件开发体验；后续可在 API 大版本中增加 MessagePack 或 Component Model adapter。

### 1.3 异步运行时

核心服务使用 Tokio。Slint UI 线程不执行阻塞 IO。后台任务通过有界 channel 将不可变快照发送给 UI 协调器。

### 1.4 迁移策略

采用旁路重写：Rust workspace 与 Swift 项目并存。Swift 版本保持可构建，直到 macOS 对齐验收通过。迁移期间不复用 Swift 二进制内部实现，只复用公开行为、配置格式和视觉基准。

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
└── legacy-swift/ (仅在后续迁移整理时考虑，不在骨架阶段移动现有文件)
```

第一阶段不移动 `Sources/`、`Package.swift`、`Scripts/` 或现有配置示例。

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

`.mip-plugin` 使用确定性 ZIP。manifest 示例：

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

核心配置继续支持 JSONC，并以 `schemaVersion` 迁移。Rust loader 分三步：

1. 去除 JSONC 注释并解析原始值。
2. 按 schema version 执行纯函数迁移。
3. 反序列化为强类型并执行跨字段验证。

迁移写入采用临时文件、fsync 和原子替换；替换前保留带时间戳备份。

插件配置存放：

```text
macOS:  ~/Library/Application Support/Mac Info Panel/plugins/<id>/
Windows: %APPDATA%/Mac Info Panel/plugins/<id>/
Linux:  $XDG_CONFIG_HOME/mac-info-panel/plugins/<id>/
```

读取旧配置时仍兼容 `~/.config/mac-info-panel/config.jsonc`。

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

迁移期间发布通道：

- Swift stable：当前用户可继续使用。
- Rust preview：使用不同 bundle ID/config root，避免覆盖稳定版。
- Rust beta：完成 macOS 对齐后提供配置迁移预览。
- Rust stable：三平台安装器和迁移通过后替代 Swift 版。

正式 macOS 产物使用 Developer ID 和公证；Windows 使用代码签名；Linux 至少提供 AppImage/Flatpak 中经验证的一种，并记录托盘和 Wayland 依赖。
