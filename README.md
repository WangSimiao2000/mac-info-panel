# Mac Info Panel

为 1280×720 Wokyis 副屏设计的原生 macOS SwiftUI/AppKit 信息面板。

## 功能

- 从 OmniLyrics 本机 API 显示歌曲、歌手、同步歌词和下一行歌词。
- 通过专用 SSH Key 和 forced command 显示 mc-ops、Minecraft 与服务器主机状态。
- 原生采集本机 CPU、内存、负载、热状态、网络、内置盘和 exSSD 状态。
- 自动寻找名为 `Wokyis` 的显示器，并兼容当前硬件 vendor/model ID。
- 无边框、无焦点、鼠标穿透，不创建 macOS 全屏 Space。
- 状态栏菜单支持显示、隐藏和退出。

## 隐私和权限

应用不申请完整磁盘访问、辅助功能、屏幕录制或管理员权限，不采集真实功耗瓦数，也不保存 mc-ops 管理员密码。

远端状态使用专用 SSH Key。服务器的 `authorized_keys` 通过 `restrict` 和 forced command 禁止完整 Shell、PTY、端口转发、Agent 转发及任意命令。collector 不输出玩家姓名、IP、日志或凭据。

## 构建

要求 macOS 14+ 和 Swift 6.2+。当前 Mac 的 Command Line Tools 已满足要求。

```bash
cd "/Volumes/exSSD/Projects/mac-info-panel"
./Scripts/build-app.sh
```

产物：

```text
.build/Mac Info Panel.app
```

直接运行：

```bash
open ".build/Mac Info Panel.app"
```

## 开机启动

该脚本将编译后的轻量 App 复制到 `~/Applications`，避免登录时外置盘还没有挂载：

```bash
./Scripts/install-launch-agent.sh
```

卸载自启动：

```bash
launchctl bootout "gui/$(id -u)/cn.mickeymiao.mac-info-panel"
rm "$HOME/Library/LaunchAgents/cn.mickeymiao.mac-info-panel.plist"
rm -rf "$HOME/Applications/Mac Info Panel.app"
```

## 配置

默认配置已经匹配当前机器。需要覆盖时：

```bash
mkdir -p "$HOME/.config/mac-info-panel"
cp config.example.json "$HOME/.config/mac-info-panel/config.json"
```

配置文件不会提交到 Git。

默认值：

```json
{
  "screenName": "Wokyis",
  "lyricsURL": "http://127.0.0.1:27270/snapshot",
  "sshHost": "mc.mickeymiao.cn",
  "sshUser": "mickeymiao",
  "sshKeyPath": "~/.ssh/id_ed25519_mc_panel",
  "sshCommand": "mcops-status-v1",
  "lyricsRefreshSeconds": 1,
  "systemRefreshSeconds": 3,
  "remoteRefreshSeconds": 15
}
```

## OmniLyrics

先启动 OmniLyrics：

```bash
open -a OmniLyrics
```

本机 API 默认为 `http://127.0.0.1:27270/snapshot`。OmniLyrics 或播放器未运行时，面板会显示等待状态，不影响其他模块。

## mc-ops 服务器接入

详细步骤见 [`server/README.md`](server/README.md)。服务器需安装：

```text
/usr/local/libexec/mcops-panel-status.py
/usr/local/libexec/mcops-panel-dispatch
```

本机需要专用密钥：

```text
~/.ssh/id_ed25519_mc_panel
```

验证命令：

```bash
ssh -T \
  -o BatchMode=yes \
  -o IdentitiesOnly=yes \
  -o ClearAllForwardings=yes \
  -i ~/.ssh/id_ed25519_mc_panel \
  mickeymiao@mc.mickeymiao.cn \
  mcops-status-v1
```

## 刷新频率与功耗

- 歌词：1 秒
- 本机状态：3 秒
- 远端 mc-ops：15 秒

窗口是静态原生 SwiftUI 视图，没有 60 FPS 游戏循环。真实瓦数需要持续运行 root 权限的 `powermetrics`，第一版有意不接入，以避免额外权限和功耗。
