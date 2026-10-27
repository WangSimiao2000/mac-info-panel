# mc-ops 只读 SSH 接入

面板不保存 mc-ops 管理员密码，也不使用拥有写权限的 Web API 会话。服务器通过专用 SSH Key 的 forced command 只返回脱敏状态 JSON。

## 服务器端安装

先通过已有管理方式登录 `mickeymiao@mc.mickeymiao.cn`，将本目录两个文件复制到服务器，然后执行：

```bash
sudo install -o root -g root -m 0755 mcops-panel-status.py /usr/local/libexec/mcops-panel-status.py
sudo install -o root -g root -m 0755 mcops-panel-dispatch /usr/local/libexec/mcops-panel-dispatch
/usr/local/libexec/mcops-panel-status.py
```

最后一条应输出单个 JSON 对象。collector 只读取：

- `http://127.0.0.1:8765/api/health`
- `mcops-agent.service` 和 `mc-server.service` 的 active 状态
- `~/minecraft/server/server.properties` 中的端口
- Minecraft 本机 Server List Ping
- `/proc` 和游戏目录所在磁盘的汇总指标

它不读取 mc-ops 密码、RCON 密码、备份凭据、玩家姓名、IP、日志或 SQLite 数据库。

## 添加面板公钥

把本机 `~/.ssh/id_ed25519_mc_panel.pub` 的公钥内容追加到服务器 `~/.ssh/authorized_keys`，并在同一行前面加：

```text
restrict,command="/usr/local/libexec/mcops-panel-dispatch"
```

完整格式：

```text
restrict,command="/usr/local/libexec/mcops-panel-dispatch" ssh-ed25519 AAAA... mac-info-panel
```

然后确认权限：

```bash
chmod 700 ~/.ssh
chmod 600 ~/.ssh/authorized_keys
```

`restrict` 会禁止 PTY、端口转发、Agent 转发、X11 和用户 rc；dispatcher 只接受固定命令 `mcops-status-v1`。

## 客户端验证

```bash
ssh -T \
  -o BatchMode=yes \
  -o IdentitiesOnly=yes \
  -o ClearAllForwardings=yes \
  -i ~/.ssh/id_ed25519_mc_panel \
  mickeymiao@mc.mickeymiao.cn \
  mcops-status-v1
```
