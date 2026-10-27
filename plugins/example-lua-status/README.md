# Example Lua Status plugin

这是一个本地 `lua@1` 示例插件。宿主不会原生执行 `plugin.lua`，而是把源码和版本化 JSON 请求交给 `plugins/lua-runner` 构建的受限 WebAssembly runner。

插件只定义 `metadata`、`validate_config` 和 `refresh`。它没有网络、文件、进程、环境、缓存或日志能力，只获准读取宿主传入的 platform 字段。

Lua 环境只提供经过 allowlist 的安全基础、数学、字符串和表操作，以及两个 JSON 辅助值：

- `json_null`：在返回表中表示 JSON `null`。
- `json_array()`：构造必须序列化为空数组的空表；非空连续整数键表会自动编码为数组。

完整 smoke test 由 `Scripts/check-rust.sh` 构建 runner 后通过真实 Extism 实例执行。
