use std::{path::PathBuf, time::Duration};

use panel_plugin_api::{ApiVersion, PluginManifest, PluginPermissions, RefreshRequest};
use panel_plugin_host::PluginHost;
use panel_plugin_runtime_extism::ExtismRuntime;

#[test]
#[ignore = "requires the example plugin to be built for wasm32-unknown-unknown"]
fn loads_and_calls_real_extism_plugin() {
    let path = std::env::var_os("INFO_PANEL_EXAMPLE_PLUGIN_WASM")
        .map(PathBuf::from)
        .expect("INFO_PANEL_EXAMPLE_PLUGIN_WASM must point to the built plugin");
    let wasm = std::fs::read(path).expect("failed to read example plugin");
    let manifest = PluginManifest {
        manifest_version: 1,
        id: "io.github.wangsimiao.example-status".into(),
        name: "Example Status".into(),
        version: "0.1.0".into(),
        api_version: ApiVersion { major: 1, minor: 0 },
        minimum_host_version: "0.1.0".into(),
        entry: "plugin.wasm".into(),
        permissions: PluginPermissions::default(),
    };
    let host = PluginHost::new(ExtismRuntime, Duration::from_secs(2));
    let mut plugin = host.load(manifest, &wasm).expect("plugin must load");
    let response = plugin
        .refresh(&RefreshRequest {
            locale: "zh-CN".into(),
            platform: "macos".into(),
            now_unix_ms: 1_798_000_000_000,
            config: serde_json::json!({}),
        })
        .expect("plugin refresh must succeed");

    assert_eq!(response.page.id.as_str(), "example.status");
    assert_eq!(response.page.title, "Example Status");
    assert_eq!(response.page.sections[0].widgets.len(), 3);
}
