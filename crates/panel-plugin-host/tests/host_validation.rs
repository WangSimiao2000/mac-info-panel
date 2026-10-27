use std::time::Duration;

use panel_model::{GaugeWidget, PageId, PageModel, Section, Widget};
use panel_plugin_api::{
    ApiVersion, PluginManifest, PluginPermissions, RefreshRequest, RefreshResponse,
};
use panel_plugin_host::PluginHost;
use panel_plugin_runtime_extism::{PluginInstance, PluginRuntime, RuntimeError};

#[derive(Clone, Copy)]
struct FakeRuntime;

struct FakePlugin;

impl PluginRuntime for FakeRuntime {
    type Instance = FakePlugin;

    fn load(&self, _wasm: &[u8], _timeout: Duration) -> Result<Self::Instance, RuntimeError> {
        Ok(FakePlugin)
    }
}

impl PluginInstance for FakePlugin {
    fn call(&mut self, _function: &str, _input: &[u8]) -> Result<Vec<u8>, RuntimeError> {
        serde_json::to_vec(&RefreshResponse {
            page: PageModel {
                schema_version: 1,
                id: PageId::new("example.status").unwrap(),
                title: "Example".into(),
                icon: None,
                sections: vec![Section {
                    title: None,
                    widgets: vec![Widget::Gauge(GaugeWidget {
                        label: "Invalid".into(),
                        value: 2.0,
                    })],
                }],
                updated_at_unix_ms: None,
            },
        })
        .map_err(|error| RuntimeError::Call(error.to_string()))
    }
}

#[test]
fn rejects_invalid_plugin_page_models() {
    let host = PluginHost::new(FakeRuntime, Duration::from_secs(1));
    let manifest = PluginManifest {
        manifest_version: 1,
        id: "io.github.example.status".into(),
        name: "Example".into(),
        version: "0.1.0".into(),
        api_version: ApiVersion { major: 1, minor: 0 },
        minimum_host_version: "0.1.0".into(),
        entry: "plugin.wasm".into(),
        permissions: PluginPermissions::default(),
    };
    let mut plugin = host.load(manifest, b"fake").unwrap();
    let request = RefreshRequest {
        locale: "zh-CN".into(),
        platform: "macos".into(),
        now_unix_ms: 0,
        config: serde_json::json!({}),
    };

    assert!(plugin.refresh(&request).is_err());
}
