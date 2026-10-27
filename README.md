# Mac Info Panel

一个面向小尺寸副屏的原生 macOS 信息面板。它使用 SwiftUI 与 AppKit，提供总览、歌词、通用 HTTP JSON 数据页和本机状态页。

## 特性

- 自动选择指定副屏，并使用 `visibleFrame` 避开菜单栏和 Dock。
- 无边框、非激活，不抢走正在使用的应用焦点。
- 鼠标位于面板时，滚轮或触控板上下/左右滑动可切换页面；主显示器上的应用不受影响。
- 状态栏菜单切页，或按配置自动轮播页面。
- 大字号、每页聚焦单一任务，并按内容使用一到三列，适合手机大小的 1280×720 小屏。
- macOS 26+ 仅在分页导航等顶层控制元素使用系统 Liquid Glass；内容区域使用系统窗口背景和语义颜色，避免把所有内容做成玻璃。
- macOS 14–15 的导航控制自动回退到原生 Material。
- 直接通过 `media-control` 读取 macOS Now Playing，并在应用内搜索、匹配和解析同步歌词。
- 首个内置歌词 provider 使用网易云 HTTPS Web API，不需要 OmniLyrics 或 .NET 运行时。
- 通过配置把任意 HTTPS JSON API 映射成状态、数值和仪表卡片。
- 原生读取 CPU、内存、负载、热状态、磁盘、网络和运行时间。
- 不需要完整磁盘访问、辅助功能、屏幕录制或管理员权限。

## 页面

默认页面类型：

- `overview`：歌词、首个网络数据源、本机状态和存储概览。
- `lyrics`：大字号歌词专页。
- `httpJSON`：一个可配置的 HTTP JSON 数据源专页。
- `localSystem`：本机资源状态专页。

页面顺序由 `config.json` 的 `pages` 数组决定。切换方式：

- 鼠标移到面板，滚轮向下或触控板向左/下滑：下一页。
- 滚轮向上或触控板向右/上滑：上一页。
- 状态栏菜单可指定页面、上一页、下一页和开关自动轮播。

面板仍是 nonactivating panel，滚动不会把它变成前台应用；但为了接收滚轮，它不再把该小屏上的鼠标事件穿透给后面的窗口。

页面采用带方向的 spring 过渡：前进从右侧进入，后退从左侧进入；歌词行单独使用垂直 spring 切换。系统启用“减少动态效果”时，两类动画都会关闭。

## UI 代码结构

```text
UI/
├── PanelTheme.swift
├── Components/
│   └── PanelComponents.swift
└── Pages/
    ├── OverviewPage.swift
    ├── LyricsPage.swift
    ├── HTTPJSONPage.swift
    └── LocalSystemPage.swift
```

`DashboardView.swift` 只保留页面路由、顶部信息和分页导航；AppKit 继续负责无焦点窗口、显示器定位和滚轮事件；SwiftUI 负责页面与组件。

## 安装

从源码安装；歌词功能只会安装轻量的 `media-control`：

```bash
git clone https://github.com/WangSimiao2000/mac-info-panel.git
cd mac-info-panel
./Scripts/install.sh
```

无人值守安装：

```bash
./Scripts/install.sh --yes
```

不安装歌词组件：

```bash
./Scripts/install.sh --without-lyrics
```

当前源码安装要求 macOS 14+ 和 Apple Command Line Tools。正式的 Developer ID 签名、公证和 Homebrew Cask 发布仍需要维护者的 Apple Developer 身份；在此之前，脚本会在本机编译并 ad-hoc 签名应用。

安装位置：

```text
~/Applications/Mac Info Panel.app
~/Library/LaunchAgents/io.github.WangSimiao2000.MacInfoPanel.plist
```

## 歌词架构

Mac Info Panel 不安装、启动或调用 OmniLyrics，也不需要 .NET 运行时。它直接启动：

```bash
media-control stream --micros --no-artwork --no-diff
```

应用使用持续子进程接收 macOS Now Playing 事件，不进行高频进程轮询；播放位置在本机单调推进，切歌后通过 generation 检查阻止迟到的歌词覆盖新歌。

歌词搜索和 LRC 解析在 Swift 进程内完成。当前内置 provider 为 `netease`，使用固定 HTTPS endpoint，并按规范化标题、首位歌手和动态时长容差筛选候选。歌词失败不会影响播放器状态、服务器页面或本机状态。

设计参考和第三方许可证见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## 配置

首次安装会把带注释的 `config.example.jsonc` 复制到：

```text
~/.config/mac-info-panel/config.jsonc
```

配置支持 `//` 行注释和 `/* ... */` 块注释；字符串中的 `https://` 不会被误当作注释。若同时存在 `config.jsonc` 和旧版 `config.json`，优先读取 JSONC。修改配置后重新启动面板。仓库中的示例不包含个人域名或凭据。

### 选择副屏

可以按名称或 EDID vendor/model 选择：

```json
"display": {
  "nameContains": "Portable Display",
  "vendorID": null,
  "modelID": null,
  "padding": 10
}
```

窗口使用目标屏幕的可见区域，再向内留出 `padding` 点，不会覆盖系统菜单栏。

### 字体与可读性

默认采用 Apple 系统字体：英文使用 SF Pro，中文由 macOS 自动选择系统中文字体。这是最接近系统应用、字形和字重最稳定的选项。

```jsonc
"appearance": {
  // 推荐：system。也可选 rounded、serif、monospaced。
  "lyricFontDesign": "system",
  // 推荐 1.0；允许 0.8–1.35。
  "lyricFontScale": 1.0
}
```

数字继续使用系统字体的 `monospacedDigit()`，只让数字等宽，不会把整块界面换成代码字体。颜色使用系统 `primary`、`secondary` 和 Accent Color；歌词正文不再固定使用蓝色或圆体。

### 页面与轮播

```json
"behavior": {
  "initialPage": "overview",
  "autoRotateSeconds": 20
}
```

将 `autoRotateSeconds` 设为 `0` 可关闭轮播。

### 通用 HTTP JSON 数据源

新安装默认不会创建 `httpJSON` 页面，`sources` 为空，也不会访问任何示例服务器。只有用户同时在 `pages` 中添加页面、在 `sources` 中添加同 ID 数据源后才会出现。

只支持 GET。默认只允许 HTTPS；HTTP 仅允许 `127.0.0.1`、`localhost` 和 `::1`。响应最大 1 MiB，使用无 Cookie 的临时会话，不执行 HTML、JavaScript、shell 或配置表达式。

示例响应：

```json
{
  "online": true,
  "users": 3,
  "host": { "cpuPercent": 27.5 },
  "uptimeSeconds": 86400
}
```

配置：

```json
{
  "id": "home-server",
  "title": "Home Server",
  "symbol": "server.rack",
  "url": "https://status.example.com/status.json",
  "refreshSeconds": 60,
  "staleAfterSeconds": 1200,
  "fields": [
    {
      "id": "online",
      "label": "Status",
      "pointer": "/online",
      "type": "boolean",
      "style": "status",
      "trueText": "Online",
      "falseText": "Offline",
      "unit": null
    },
    {
      "id": "cpu",
      "label": "CPU",
      "pointer": "/host/cpuPercent",
      "type": "percent",
      "style": "gauge",
      "trueText": null,
      "falseText": null,
      "unit": null
    }
  ]
}
```

字段路径使用 RFC 6901 JSON Pointer。支持的 `type`：

```text
string, number, boolean, percent, bytes, duration, date
```

支持的 `style`：

```text
value, status, gauge
```

一个数据源失败不会影响其他页面；面板保留最后一次成功结果并标记错误或过期。用户可以自定义 URL、刷新周期、页面标题、SF Symbol、字段标签、JSON Pointer、显示类型、样式、单位以及布尔值文案，但不能配置任意脚本或计算表达式。

可复制的可选数据源片段位于：

```text
Examples/http-json-page.jsonc
```

### 磁盘卷

```json
"volumes": [
  { "id": "system", "label": "System", "path": "/" },
  { "id": "projects", "label": "Projects", "path": "/Volumes/Projects" }
]
```

## 内置歌词

安装器只安装 Homebrew `media-control`：

```bash
brew install media-control
```

默认配置：

```json
"lyrics": {
  "enabled": true,
  "mediaControlPath": null,
  "provider": "netease"
}
```

`mediaControlPath` 为空时依次查找 `/opt/homebrew/bin/media-control` 和 `/usr/local/bin/media-control`。播放器未运行、歌词未匹配或歌词服务暂时不可用时，歌词页显示明确状态，其他页面继续工作。

## 构建与开发

```bash
./Scripts/build-app.sh
open ".build/Mac Info Panel.app"
```

重新安装并刷新 LaunchAgent：

```bash
./Scripts/install-launch-agent.sh
```

## 可选的 mc-ops SSH collector 示例

通用面板推荐直接消费只读 HTTPS JSON。如果现有服务只有本地接口，可以参考 [`server/README.md`](server/README.md)，用独立 SSH Key、`restrict` 和 forced command 暴露最小 JSON；不要把管理员 Cookie、RCON 或完整 Shell 交给面板。

## 卸载

```bash
launchctl bootout "gui/$(id -u)/io.github.WangSimiao2000.MacInfoPanel"
rm "$HOME/Library/LaunchAgents/io.github.WangSimiao2000.MacInfoPanel.plist"
rm -rf "$HOME/Applications/Mac Info Panel.app"
```

配置位于 `~/.config/mac-info-panel`，卸载应用时不会自动删除。

## License

MIT
