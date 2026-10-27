# 可选：用受限 SSH 暴露只读 JSON

通用面板优先使用 HTTPS JSON API。本目录仅提供一个 Minecraft/mc-ops 示例，适合服务只监听服务器回环地址且不希望暴露管理员 API 的情况。

## 安全模型

- 独立 ED25519 Key。
- `restrict` 禁止 PTY、端口转发、Agent 转发、X11 和用户 rc。
- forced command 只接受固定的 `mcops-status-v1`。
- collector 不接收参数，不执行客户端输入。
- 不读取管理员密码、RCON、备份凭据、玩家姓名、IP、日志或数据库。

## 服务器端安装

将两个文件复制到服务器，然后由管理员执行：

```bash
sudo install -o root -g root -m 0755 mcops-panel-status.py /usr/local/libexec/mcops-panel-status.py
sudo install -o root -g root -m 0755 mcops-panel-dispatch /usr/local/libexec/mcops-panel-dispatch
/usr/local/libexec/mcops-panel-status.py
```

collector 的示例假定：

```text
mc-ops health: http://127.0.0.1:8765/api/health
systemd user units: mcops-agent.service, mc-server.service
Minecraft directory: ~/minecraft/server
```

请在部署前按自己的环境审查并修改固定路径。

## authorized_keys

将面板公钥加入运行 mc-ops 的 Linux 用户 `~/.ssh/authorized_keys`：

```text
restrict,command="/usr/local/libexec/mcops-panel-dispatch" ssh-ed25519 AAAA... mac-info-panel
```

确认权限：

```bash
chmod 700 ~/.ssh
chmod 600 ~/.ssh/authorized_keys
```

## 客户端验证

```bash
ssh -T \
  -o BatchMode=yes \
  -o IdentitiesOnly=yes \
  -o ClearAllForwardings=yes \
  -i ~/.ssh/id_ed25519_mc_panel \
  user@example.com \
  mcops-status-v1
```

该示例输出可以通过 HTTPS 反向代理或静态发布给通用 `httpJSON` 数据源；公开时请继续避免加入私密字段。
