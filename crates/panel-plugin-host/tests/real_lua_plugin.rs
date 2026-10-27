use std::{path::PathBuf, time::Duration};

use panel_plugin_api::{PackageIdentity, PermissionGrant, PluginManifest};
use panel_plugin_host::{HostLimits, PluginHost, PluginPackage, RefreshContext, RefreshMode};
use panel_plugin_runtime_extism::{LuaWasmRuntime, RuntimeLimits};

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn loads_and_calls_real_lua_plugin_through_wasm_runner() {
    let runner_path = std::env::var_os("INFO_PANEL_LUA_RUNNER_WASM")
        .map(PathBuf::from)
        .expect("INFO_PANEL_LUA_RUNNER_WASM must point to the built runner");
    let runtime = LuaWasmRuntime::new(
        std::fs::read(runner_path).expect("failed to read restricted Lua runner"),
    )
    .expect("runner must not be empty");
    let manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../plugins/example-lua-status/manifest.json"
    ))
    .expect("example Lua manifest must deserialize");
    let package = PluginPackage {
        identity: PackageIdentity {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            entry: manifest.entry.clone(),
        },
        entry: manifest.entry.clone(),
        bytes: include_bytes!("../../../plugins/example-lua-status/plugin.lua").to_vec(),
    };
    let grant = PermissionGrant {
        plugin_id: manifest.id.clone(),
        permissions: manifest.permissions.clone(),
    };
    let host = PluginHost::new(
        runtime,
        HostLimits {
            runtime: RuntimeLimits {
                execution_timeout: Duration::from_secs(2),
                max_memory_bytes: 64 * 1024 * 1024,
                max_output_bytes: 1024 * 1024,
                max_error_bytes: 8 * 1024,
                max_instructions: 250_000,
                max_input_bytes: 1024 * 1024,
                max_lua_memory_bytes: 16 * 1024 * 1024,
                max_lua_stack_depth: 128,
                max_host_calls: 0,
            },
            ..HostLimits::default()
        },
    )
    .expect("host limits must be valid");
    let mut plugin = host
        .load(manifest, package, &grant)
        .expect("Lua plugin must load through the runner");
    assert!(
        plugin
            .validate_config_json(b"{}")
            .expect("configuration ABI must succeed")
            .valid
    );
    let result = plugin
        .refresh(
            RefreshContext {
                locale: "zh-CN".into(),
                platform: "macos".into(),
                now_unix_ms: 1_798_000_000_000,
            },
            Duration::ZERO,
            RefreshMode::Scheduled,
        )
        .expect("Lua refresh must succeed");

    assert_eq!(result.generation, 1);
    assert_eq!(result.response.page.id.as_str(), "example.lua.status");
    assert_eq!(result.response.page.title, "Example Lua Status");
    assert_eq!(result.response.page.sections[0].widgets.len(), 3);
}
