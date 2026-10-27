# Mac Info Panel

一个面向小尺寸副屏的原生 macOS 信息面板。它使用 SwiftUI 与 AppKit，提供总览、歌词、通用 HTTP JSON 数据页和本机状态页。

## 特性

- 自动选择指定副屏，并使用 `visibleFrame` 避开菜单栏和 Dock。
- 无边框、非激活、鼠标穿透，不抢走正在使用的应用焦点。
- 状态栏菜单切页，或按配置自动轮播页面。
- 大字号、两列以内布局，适合手机大小的 1280×720 小屏。
- macOS 26+ 使用系统 Liquid Glass，macOS 14–15 回退到原生 Material。
- 从 OmniLyrics 本机 API 显示歌曲、进度、当前/下一行歌词，并按播放时钟实时推进。
- 安装器同时配置 `media-control`，让网易云音乐等系统 Now Playing 播放器可被识别。
- 通过配置把任意 HTTPS JSON API 映射成状态、数值和仪表卡片。
- 原生读取 CPU、内存、负载、热状态、磁盘、网络和运行时间。
- 不需要完整磁盘访问、辅助功能、屏幕录制或管理员权限。

## 页面

默认页面类型：

- `overview`：歌词、首个网络数据源、本机状态和存储概览。
- `lyrics`：大字号歌词专页。
- `httpJSON`：一个可配置的 HTTP JSON 数据源专页。
- `localSystem`：本机资源状态专页。

页面顺序由 `config.json` 的 `pages` 数组决定。状态栏菜单支持指定页面、上一页、下一页和自动轮播。

## 安装

从源码安装主应用并按需安装 OmniLyrics：

```bash
git clone https://github.com/WangSimiao2000/mac-info-panel.git
cd mac-info-panel
./Scripts/install.sh
```

无人值守接受 OmniLyrics 安装：

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

## 为什么不把 OmniLyrics 做成子模块

OmniLyrics 是独立应用，面板只依赖其版本化本机 HTTP API。安装脚本会检测签名者并在需要时通过维护者的 Homebrew Cask 安装它，因此用户仍可一条命令完成安装，同时两个项目可以独立升级。源码子模块不会提供可执行程序，反而会要求用户安装 .NET SDK、编译并维护两个生命周期。

OmniLyrics 项目：<https://github.com/zzxzzk115/OmniLyrics>

## 配置

首次安装会把 `config.example.json` 复制到：

```text
~/.config/mac-info-panel/config.json
```

修改配置后重新启动面板。仓库中的示例不包含个人域名或凭据。

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

### 页面与轮播

```json
"behavior": {
  "initialPage": "overview",
  "autoRotateSeconds": 20
}
```

将 `autoRotateSeconds` 设为 `0` 可关闭轮播。

### 通用 HTTP JSON 数据源

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

一个数据源失败不会影响其他页面；面板保留最后一次成功结果并标记错误或过期。

### 磁盘卷

```json
"volumes": [
  { "id": "system", "label": "System", "path": "/" },
  { "id": "projects", "label": "Projects", "path": "/Volumes/Projects" }
]
```

## OmniLyrics

默认接口：

```text
http://127.0.0.1:27270/snapshot
```

安装器可通过以下官方维护者 Cask 安装 OmniLyrics，并安装 Homebrew `media-control` 以支持网易云音乐等 macOS Now Playing 播放器：

```bash
brew install --cask zzxzzk115/tap/omnilyrics
```

面板默认以不激活方式启动 OmniLyrics。可以在配置中关闭：

```json
"autoLaunch": false
```

OmniLyrics 缺失或播放器未运行时，歌词页显示等待状态，其他页面继续工作。

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
