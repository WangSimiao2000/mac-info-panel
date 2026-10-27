use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use panel_model::{GaugeWidget, PageId, PageModel, Section, Widget};
use panel_plugin_api::{
    ApiVersion, ConfigDiagnostic, MetadataResponse, PackageIdentity, PermissionGrant,
    PluginManifest, PluginPageMetadata, PluginPermissions, PluginRuntimeKind, RefreshResponse,
    ValidateConfigResponse,
};
use panel_plugin_host::{
    BackoffPolicy, HostError, HostLimits, JitterSource, LifecycleState, PluginHost,
    PluginLifecycle, PluginPackage, RefreshContext, RefreshMode, SystemJitter,
};
use panel_plugin_runtime_extism::{
    PluginCancellation, PluginInstance, PluginRuntime, RuntimeError, RuntimeLimits,
};

#[derive(Clone)]
struct FakeRuntime {
    mode: FakeMode,
}

#[derive(Clone)]
enum FakeMode {
    InvalidPage,
    Valid,
    MetadataMismatch,
    MalformedMetadata,
    InvalidUtf8Metadata,
    OversizedDiagnostic,
    SecretDiagnostic,
    InvalidDiagnosticPath,
    RefreshFailure,
    Panic,
    LoadPanic,
    CancellationHandlePanic,
    CancelPanic,
}

struct FakePlugin {
    mode: FakeMode,
}

#[derive(Clone)]
struct FakeCancellation {
    cancelled: Arc<AtomicBool>,
    panic_on_cancel: bool,
}

impl PluginCancellation for FakeCancellation {
    fn cancel(&self) -> Result<(), RuntimeError> {
        if self.panic_on_cancel {
            panic!("cancel-secret-payload");
        }
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl PluginRuntime for FakeRuntime {
    type Instance = FakePlugin;

    fn load(&self, _wasm: &[u8], _limits: RuntimeLimits) -> Result<Self::Instance, RuntimeError> {
        if matches!(self.mode, FakeMode::LoadPanic) {
            panic!("load-secret-payload");
        }
        Ok(FakePlugin {
            mode: self.mode.clone(),
        })
    }
}

impl PluginInstance for FakePlugin {
    type Cancellation = FakeCancellation;

    fn cancellation_handle(&self) -> Self::Cancellation {
        if matches!(self.mode, FakeMode::CancellationHandlePanic) {
            panic!("handle-secret-payload");
        }
        FakeCancellation {
            cancelled: Arc::new(AtomicBool::new(false)),
            panic_on_cancel: matches!(self.mode, FakeMode::CancelPanic),
        }
    }

    fn call(
        &mut self,
        function: &str,
        _input: &[u8],
        cancellation: &Self::Cancellation,
    ) -> Result<Vec<u8>, RuntimeError> {
        if cancellation.cancelled.load(Ordering::SeqCst) {
            return Err(RuntimeError::Cancelled);
        }
        match function {
            "metadata" => match self.mode {
                FakeMode::MetadataMismatch => serde_json::to_vec(&MetadataResponse {
                    id: "io.github.attacker.other".into(),
                    ..metadata()
                })
                .map_err(|_| RuntimeError::CallFailed),
                FakeMode::MalformedMetadata => Ok(b"{".to_vec()),
                FakeMode::InvalidUtf8Metadata => Ok(vec![0xff, 0xfe]),
                _ => serde_json::to_vec(&metadata()).map_err(|_| RuntimeError::CallFailed),
            },
            "validate_config" if matches!(self.mode, FakeMode::Panic) => {
                panic!("must-not-format")
            }
            "validate_config" => {
                let diagnostics = if matches!(self.mode, FakeMode::OversizedDiagnostic) {
                    vec![ConfigDiagnostic {
                        code: "invalid".into(),
                        message: "x".repeat(513),
                        path: None,
                    }]
                } else if matches!(self.mode, FakeMode::SecretDiagnostic) {
                    vec![ConfigDiagnostic {
                        code: "invalid.token".into(),
                        message: "token is super-secret-value".into(),
                        path: Some("/token".into()),
                    }]
                } else if matches!(self.mode, FakeMode::InvalidDiagnosticPath) {
                    vec![ConfigDiagnostic {
                        code: "invalid.token".into(),
                        message: "invalid".into(),
                        path: Some("/super-secret-value".into()),
                    }]
                } else {
                    Vec::new()
                };
                serde_json::to_vec(&ValidateConfigResponse {
                    valid: diagnostics.is_empty(),
                    diagnostics,
                })
                .map_err(|_| RuntimeError::CallFailed)
            }
            "refresh" if matches!(self.mode, FakeMode::RefreshFailure) => {
                Err(RuntimeError::CallFailed)
            }
            "refresh" => serde_json::to_vec(&RefreshResponse {
                page: page(
                    matches!(self.mode, FakeMode::InvalidPage)
                        .then_some(2.0)
                        .unwrap_or(0.5),
                ),
            })
            .map_err(|_| RuntimeError::CallFailed),
            _ => Err(RuntimeError::CallFailed),
        }
    }
}

fn page_metadata() -> PluginPageMetadata {
    PluginPageMetadata {
        id: "example.status".into(),
        title: "Example".into(),
        icon: None,
    }
}

fn metadata() -> MetadataResponse {
    MetadataResponse {
        id: "io.github.example.status".into(),
        version: "0.1.0".into(),
        api_version: ApiVersion { major: 1, minor: 0 },
        runtime: PluginRuntimeKind::WasmV1,
        pages: vec![page_metadata()],
    }
}

fn manifest() -> PluginManifest {
    PluginManifest {
        manifest_version: 1,
        id: "io.github.example.status".into(),
        name: "Example".into(),
        version: "0.1.0".into(),
        api_version: ApiVersion { major: 1, minor: 0 },
        minimum_host_version: "0.1.0".into(),
        runtime: PluginRuntimeKind::WasmV1,
        entry: "plugin.wasm".into(),
        pages: vec![page_metadata()],
        permissions: PluginPermissions::default(),
        source: "https://example.com/source".into(),
        license: "MIT".into(),
    }
}

fn package(manifest: &PluginManifest) -> PluginPackage {
    PluginPackage {
        identity: PackageIdentity {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            entry: manifest.entry.clone(),
        },
        entry: manifest.entry.clone(),
        bytes: b"fake".to_vec(),
    }
}

fn grant(manifest: &PluginManifest) -> PermissionGrant {
    PermissionGrant {
        plugin_id: manifest.id.clone(),
        permissions: manifest.permissions.clone(),
    }
}

fn page(value: f64) -> PageModel {
    PageModel {
        schema_version: 1,
        id: PageId::new("example.status").unwrap(),
        title: "Example".into(),
        icon: None,
        sections: vec![Section {
            title: None,
            widgets: vec![Widget::Gauge(GaugeWidget {
                label: "Load".into(),
                value,
            })],
        }],
        updated_at_unix_ms: None,
    }
}

fn load(mode: FakeMode) -> panel_plugin_host::LoadedPlugin<FakePlugin> {
    let manifest = manifest();
    PluginHost::new(FakeRuntime { mode }, HostLimits::default())
        .unwrap()
        .load(manifest.clone(), package(&manifest), &grant(&manifest))
        .unwrap()
}

fn context() -> RefreshContext {
    RefreshContext {
        locale: "zh-CN".into(),
        platform: "macos".into(),
        now_unix_ms: 0,
    }
}

#[test]
fn rejects_invalid_plugin_page_models_after_deserialization() {
    let mut plugin = load(FakeMode::InvalidPage);
    plugin.validate_config_json(b"{}").unwrap();
    assert!(matches!(
        plugin.refresh(context(), Duration::ZERO, RefreshMode::Scheduled),
        Err(HostError::RefreshFailed { .. })
    ));
    assert_eq!(plugin.diagnostics()[0].code, "plugin.invalid-page");
}

#[test]
fn validates_package_identity_and_plugin_metadata() {
    let manifest = manifest();
    let host = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::Valid,
        },
        HostLimits::default(),
    )
    .unwrap();
    let mut wrong_package = package(&manifest);
    wrong_package.identity.id = "io.github.attacker.other".into();
    assert!(matches!(
        host.load(manifest.clone(), wrong_package, &grant(&manifest)),
        Err(HostError::PackageIdentityMismatch)
    ));

    let host = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::MetadataMismatch,
        },
        HostLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        host.load(manifest.clone(), package(&manifest), &grant(&manifest)),
        Err(HostError::MetadataMismatch)
    ));

    for json in [
        include_str!("../../panel-plugin-api/tests/fixtures/abi-future-minor.json"),
        include_str!("../../panel-plugin-api/tests/fixtures/abi-different-major.json"),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut incompatible = manifest.clone();
        incompatible.api_version = serde_json::from_value(fixture["apiVersion"].clone()).unwrap();
        assert!(matches!(
            host.load(
                incompatible.clone(),
                package(&incompatible),
                &grant(&incompatible)
            ),
            Err(HostError::Manifest(
                panel_plugin_api::ManifestError::IncompatibleApi { .. }
            ))
        ));
    }
}

#[test]
fn rejects_ungranted_and_undeclared_network_origins() {
    let mut manifest = manifest();
    manifest.permissions.network = vec!["https://declared.example".into()];
    let ungranted = PermissionGrant {
        plugin_id: manifest.id.clone(),
        permissions: PluginPermissions::default(),
    };
    let host = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::Valid,
        },
        HostLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        host.load(manifest.clone(), package(&manifest), &ungranted),
        Err(HostError::PermissionsNotGranted(_))
    ));

    let mut grant = grant(&manifest);
    grant.permissions.network = vec!["https://undeclared.example".into()];
    assert!(matches!(
        host.load(manifest.clone(), package(&manifest), &grant),
        Err(HostError::PermissionsNotGranted(_))
    ));
}

#[test]
fn refuses_lua_without_a_restricted_wasm_runner() {
    let mut manifest = manifest();
    manifest.runtime = PluginRuntimeKind::LuaV1;
    manifest.entry = "plugin.lua".into();
    let package = package(&manifest);
    assert!(matches!(
        PluginHost::new(
            FakeRuntime {
                mode: FakeMode::Valid
            },
            HostLimits::default()
        )
        .unwrap()
        .load(manifest.clone(), package, &grant(&manifest)),
        Err(HostError::LuaRunnerUnavailable)
    ));
}

#[test]
fn rejects_malformed_json_invalid_utf8_and_unbounded_diagnostics() {
    for mode in [FakeMode::MalformedMetadata, FakeMode::InvalidUtf8Metadata] {
        let manifest = manifest();
        assert!(matches!(
            PluginHost::new(FakeRuntime { mode }, HostLimits::default())
                .unwrap()
                .load(manifest.clone(), package(&manifest), &grant(&manifest)),
            Err(HostError::InvalidOutput)
        ));
    }

    let mut plugin = load(FakeMode::OversizedDiagnostic);
    assert!(matches!(
        plugin.validate_config_json(b"{}"),
        Err(HostError::InvalidOutput)
    ));

    let mut manifest = manifest();
    manifest.permissions.config = true;
    let mut plugin = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::SecretDiagnostic,
        },
        HostLimits::default(),
    )
    .unwrap()
    .load(manifest.clone(), package(&manifest), &grant(&manifest))
    .unwrap();
    let response = plugin
        .validate_config_json(br#"{"token":"super-secret-value"}"#)
        .expect("bounded invalid configuration response must be accepted");
    assert!(!response.valid);
    assert_eq!(
        response.diagnostics[0].message,
        "Plugin configuration is invalid"
    );
    assert!(
        !response.diagnostics[0]
            .message
            .contains("super-secret-value")
    );
}

#[test]
fn plugin_failures_are_isolated_and_do_not_echo_configuration() {
    let mut plugin = load(FakeMode::RefreshFailure);
    plugin.validate_config_json(b"{}").unwrap();
    let error = plugin
        .refresh(context(), Duration::ZERO, RefreshMode::Scheduled)
        .unwrap_err();
    assert!(matches!(error, HostError::RefreshFailed { .. }));
    assert!(!error.to_string().contains("config"));
    assert_eq!(plugin.diagnostics().len(), 1);
}

#[test]
fn config_bytes_are_limited_before_json_allocation() {
    let mut manifest = manifest();
    manifest.permissions.config = true;
    let mut plugin = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::Valid,
        },
        HostLimits {
            max_input_bytes: 128,
            ..HostLimits::default()
        },
    )
    .unwrap()
    .load(manifest.clone(), package(&manifest), &grant(&manifest))
    .unwrap();
    assert!(matches!(
        plugin.validate_config_json(&[b' '; 129]),
        Err(HostError::InputTooLarge)
    ));
    let config = format!(r#"{{"token":"{}"}}"#, "x".repeat(110));
    assert!(config.len() <= 128);
    assert!(matches!(
        plugin.validate_config_json(config.as_bytes()),
        Err(HostError::InputTooLarge)
    ));
}

#[test]
fn rejects_diagnostic_paths_that_do_not_resolve_in_config() {
    let mut manifest = manifest();
    manifest.permissions.config = true;
    let mut plugin = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::InvalidDiagnosticPath,
        },
        HostLimits::default(),
    )
    .unwrap()
    .load(manifest.clone(), package(&manifest), &grant(&manifest))
    .unwrap();
    assert!(matches!(
        plugin.validate_config_json(br#"{"token":"super-secret-value"}"#),
        Err(HostError::InvalidOutput)
    ));
}

#[test]
fn runtime_panics_invalidate_instance_without_formatting_payload() {
    let mut plugin = load(FakeMode::Panic);
    let first = plugin.validate_config_json(b"{}").unwrap_err();
    assert!(matches!(first, HostError::RuntimePanicked));
    assert!(!first.to_string().contains("must-not-format"));
    let second = plugin.validate_config_json(b"{}").unwrap_err();
    assert!(matches!(second, HostError::InstanceInvalid));

    let load_manifest = manifest();
    let load_error = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::LoadPanic,
        },
        HostLimits::default(),
    )
    .unwrap()
    .load(
        load_manifest.clone(),
        package(&load_manifest),
        &grant(&load_manifest),
    )
    .err()
    .expect("load panic must be converted");
    assert!(matches!(load_error, HostError::RuntimePanicked));
    assert!(!load_error.to_string().contains("load-secret-payload"));

    let manifest = manifest();
    let handle_error = PluginHost::new(
        FakeRuntime {
            mode: FakeMode::CancellationHandlePanic,
        },
        HostLimits::default(),
    )
    .unwrap()
    .load(manifest.clone(), package(&manifest), &grant(&manifest))
    .err()
    .expect("cancellation handle panic must be converted");
    assert!(matches!(handle_error, HostError::RuntimePanicked));
    assert!(!handle_error.to_string().contains("handle-secret-payload"));

    let mut cancel_plugin = load(FakeMode::CancelPanic);
    cancel_plugin.validate_config_json(b"{}").unwrap();
    let pending = cancel_plugin
        .begin_refresh(Duration::ZERO, RefreshMode::Scheduled)
        .unwrap();
    assert!(matches!(
        pending.cancellation_handle().cancel(),
        Err(HostError::RuntimePanicked)
    ));
    assert!(matches!(
        cancel_plugin.validate_config_json(b"{}"),
        Err(HostError::InstanceInvalid)
    ));
}

#[test]
fn production_jitter_stays_in_range_and_desynchronizes_plugins() {
    let policy = BackoffPolicy {
        initial: Duration::from_secs(4),
        maximum: Duration::from_secs(30),
        jitter_ratio_percent: 25,
    };
    let source = SystemJitter::default();
    let mut first = PluginLifecycle::new(policy, source.clone()).unwrap();
    let mut second = PluginLifecycle::new(policy, source).unwrap();
    let first_generation = first.begin(Duration::ZERO, RefreshMode::Scheduled).unwrap();
    let second_generation = second
        .begin(Duration::ZERO, RefreshMode::Scheduled)
        .unwrap();
    let first_retry = first
        .fail(first_generation, Duration::ZERO, RefreshMode::Scheduled)
        .unwrap();
    let second_retry = second
        .fail(second_generation, Duration::ZERO, RefreshMode::Scheduled)
        .unwrap();
    for retry in [first_retry, second_retry] {
        assert!(retry >= Duration::from_secs(4));
        assert!(retry <= Duration::from_secs(5));
    }
    assert_ne!(first_retry, second_retry);
}

#[derive(Clone)]
struct FixedJitter(Duration);

impl JitterSource for FixedJitter {
    fn sample(&mut self, upper_bound: Duration) -> Duration {
        self.0.min(upper_bound)
    }
}

#[test]
fn lifecycle_uses_generation_backoff_deterministic_jitter_and_one_manual_retry() {
    let mut lifecycle = PluginLifecycle::new(
        BackoffPolicy {
            initial: Duration::from_secs(4),
            maximum: Duration::from_secs(30),
            jitter_ratio_percent: 25,
        },
        FixedJitter(Duration::from_secs(1)),
    )
    .unwrap();
    let first = lifecycle
        .begin(Duration::ZERO, RefreshMode::Scheduled)
        .unwrap();
    assert_eq!(
        lifecycle
            .fail(first, Duration::ZERO, RefreshMode::Scheduled)
            .unwrap(),
        Duration::from_secs(5)
    );
    assert!(lifecycle.manual_retry_available());
    assert!(matches!(
        lifecycle.begin(Duration::from_secs(1), RefreshMode::Scheduled),
        Err(HostError::BackingOff { .. })
    ));

    let manual = lifecycle
        .begin(Duration::from_secs(1), RefreshMode::ManualRetry)
        .unwrap();
    assert_eq!(
        lifecycle
            .fail(manual, Duration::from_secs(1), RefreshMode::ManualRetry)
            .unwrap(),
        Duration::from_secs(10)
    );
    assert!(!lifecycle.manual_retry_available());
    assert!(matches!(
        lifecycle.begin(Duration::from_secs(2), RefreshMode::ManualRetry),
        Err(HostError::ManualRetryUnavailable)
    ));

    let scheduled = lifecycle
        .begin(Duration::from_secs(11), RefreshMode::Scheduled)
        .unwrap();
    lifecycle.cancel();
    assert_eq!(lifecycle.state(), LifecycleState::Cancelled);
    assert!(matches!(
        lifecycle.succeed(scheduled),
        Err(HostError::StaleGeneration)
    ));
}

#[test]
fn cancellation_handle_invalidates_only_its_pending_generation() {
    let mut plugin = load(FakeMode::Valid);
    plugin.validate_config_json(b"{}").unwrap();
    let pending = plugin
        .begin_refresh(Duration::ZERO, RefreshMode::Scheduled)
        .unwrap();
    let cancellation = pending.cancellation_handle();
    cancellation.cancel().unwrap();
    assert_eq!(plugin.lifecycle_state(), LifecycleState::Cancelled);
    assert!(matches!(
        plugin.execute_refresh(pending, context(), Duration::ZERO),
        Err(HostError::StaleGeneration)
    ));
}
