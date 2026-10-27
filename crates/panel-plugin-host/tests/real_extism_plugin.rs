use std::{path::PathBuf, time::Duration};

use panel_plugin_api::{
    ApiVersion, PackageIdentity, PermissionGrant, PluginManifest, PluginPageMetadata,
    PluginPermissions, PluginRuntimeKind,
};
use panel_plugin_host::{HostLimits, PluginHost, PluginPackage, RefreshContext, RefreshMode};
use panel_plugin_runtime_extism::{ExtismRuntime, RuntimeLimits};

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
        runtime: PluginRuntimeKind::WasmV1,
        entry: "plugin.wasm".into(),
        pages: vec![PluginPageMetadata {
            id: "example.status".into(),
            title: "Example Status".into(),
            icon: Some("gauge".into()),
        }],
        permissions: PluginPermissions {
            platform: true,
            ..PluginPermissions::default()
        },
        source: "https://github.com/WangSimiao2000/mac-info-panel".into(),
        license: "MIT".into(),
    };
    let package = PluginPackage {
        identity: PackageIdentity {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            entry: manifest.entry.clone(),
        },
        entry: manifest.entry.clone(),
        bytes: wasm,
    };
    let grant = PermissionGrant {
        plugin_id: manifest.id.clone(),
        permissions: manifest.permissions.clone(),
    };
    let host = PluginHost::new(
        ExtismRuntime,
        HostLimits {
            runtime: RuntimeLimits {
                execution_timeout: Duration::from_secs(2),
                max_memory_bytes: 64 * 1024 * 1024,
                max_output_bytes: 1024 * 1024,
                max_error_bytes: 8 * 1024,
                ..RuntimeLimits::default()
            },
            ..HostLimits::default()
        },
    )
    .unwrap();
    let mut plugin = host
        .load(manifest, package, &grant)
        .expect("plugin must load");
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
        .expect("plugin refresh must succeed");

    assert_eq!(result.generation, 1);
    assert_eq!(result.response.page.id.as_str(), "example.status");
    assert_eq!(result.response.page.title, "Example Status");
    assert_eq!(result.response.page.sections[0].widgets.len(), 3);
}
