# Mac Info Panel

原生 macOS 小屏信息面板，使用 SwiftUI + AppKit 构建，适合把便携屏、Sidecar 或副显示器用作常驻 Dashboard。

## 功能

- 总览、同步歌词、本机状态和可选 HTTP JSON 页面
- 自动选择副屏并避开菜单栏和 Dock
- 滚轮、触控板、状态栏菜单和自动轮播切页
- Spring 页面/歌词动效，支持“减少动态效果”
- macOS 26 Liquid Glass，旧系统使用原生 Material
- CPU、内存、磁盘、网络、负载和热状态
- 通过 `media-control` 读取 macOS Now Playing，内置网易云 LRC 歌词
- 不依赖 OmniLyrics、.NET、完整磁盘访问、辅助功能或屏幕录制

## 要求

- macOS 14+
- Apple Command Line Tools
- Homebrew（启用歌词时用于安装 `media-control`）

## 安装

```bash
git clone https://github.com/WangSimiao2000/mac-info-panel.git
cd mac-info-panel
./Scripts/install.sh
```

不安装歌词依赖：

```bash
./Scripts/install.sh --without-lyrics
```

应用安装到 `~/Applications/Mac Info Panel.app`，并创建登录自启动 LaunchAgent。

> 当前从源码编译并使用本机 ad-hoc 签名；正式发行仍需 Developer ID 签名和 Apple 公证。

## 使用

- 滚轮向下或触控板向左/下滑：下一页
- 滚轮向上或触控板向右/上滑：上一页
- 状态栏菜单：指定页面、开关自动轮播、显示/隐藏和退出

面板是 nonactivating panel，切页不会抢走当前应用的焦点。为了接收滚轮，小屏上的鼠标事件不会穿透给后方窗口。

## 配置

带注释的用户配置：

```text
~/.config/mac-info-panel/config.jsonc
```

完整说明和默认值见 [`config.example.jsonc`](config.example.jsonc)。若同时存在 `config.jsonc` 与旧版 `config.json`，优先读取 JSONC。

默认只显示总览、歌词和本机状态，不会请求任何自定义状态 API。添加 HTTP JSON 页面可复制：

```text
Examples/http-json-page.jsonc
```

HTTP JSON 数据源支持：

- RFC 6901 JSON Pointer
- `string`、`number`、`boolean`、`percent`、`bytes`、`duration`、`date`
- `value`、`status`、`gauge` 三种样式
- 自定义标题、SF Symbol、刷新周期、单位和布尔文案

数据源只执行 GET；默认只允许 HTTPS，HTTP 仅限本机回环地址；不执行脚本或表达式，不保存 Cookie，响应上限为 1 MiB。

字体默认使用 Apple 系统字体。歌词可选 `system`、`rounded`、`serif`、`monospaced`，并可设置 `0.8–1.35` 的字号倍率。

修改配置后重启：

```bash
launchctl kickstart -k "gui/$(id -u)/io.github.WangSimiao2000.MacInfoPanel"
```

## 开发

```bash
./Scripts/build-app.sh
open ".build/Mac Info Panel.app"
```

UI 代码按主题、组件和页面拆分：

```text
Sources/MacInfoPanel/UI/
├── PanelTheme.swift
├── Components/
└── Pages/
```

只读 SSH collector 示例位于 [`server/README.md`](server/README.md)。第三方项目和接口声明见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## 卸载

```bash
launchctl bootout "gui/$(id -u)/io.github.WangSimiao2000.MacInfoPanel"
rm "$HOME/Library/LaunchAgents/io.github.WangSimiao2000.MacInfoPanel.plist"
rm -rf "$HOME/Applications/Mac Info Panel.app"
```

用户配置不会自动删除。

## 致谢

歌词播放时钟、曲目匹配边界、缓存和切歌竞态设计参考了 [OmniLyrics v0.4.2](https://github.com/zzxzzk115/OmniLyrics)。Mac Info Panel 不包含或依赖 OmniLyrics；完整版权与许可声明见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## License

[MIT](LICENSE)
