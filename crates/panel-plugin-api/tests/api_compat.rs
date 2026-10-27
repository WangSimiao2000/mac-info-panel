use panel_plugin_api::{
    ApiVersion, MetadataRequest, MetadataResponse, RefreshRequest, RefreshResponse,
    ValidateConfigRequest, ValidateConfigResponse,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const HOST_API: ApiVersion = ApiVersion { major: 1, minor: 2 };

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AbiFixture {
    api_version: ApiVersion,
    metadata_request: Value,
    metadata_response: Value,
    validate_config_request: Value,
    validate_config_response: Value,
    refresh_request: Value,
    refresh_response: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct V1MetadataRequest {
    host_api_version: ApiVersion,
    host_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct V1ValidateConfigRequest {
    config: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct V1RefreshRequest {
    generation: u64,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    now_unix_ms: i64,
    #[serde(default)]
    config: Option<Value>,
}

fn fixture(json: &str) -> AbiFixture {
    serde_json::from_str(json).expect("ABI fixture must deserialize")
}

fn decode<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("compatible ABI message must deserialize")
}

fn assert_unknown_field_rejected<T: DeserializeOwned>(mut value: Value) {
    value
        .as_object_mut()
        .expect("ABI message must be an object")
        .insert("unknownField".into(), Value::Bool(true));
    assert!(serde_json::from_value::<T>(value).is_err());
}

#[test]
fn accepts_every_current_json_abi_message() {
    let value = fixture(include_str!("fixtures/abi-current.json"));
    assert_eq!(value.api_version, HOST_API);
    assert!(value.api_version.is_compatible_with(HOST_API));

    let _: MetadataRequest = decode(value.metadata_request);
    let metadata: MetadataResponse = decode(value.metadata_response);
    let _: ValidateConfigRequest = decode(value.validate_config_request);
    let _: ValidateConfigResponse = decode(value.validate_config_response);
    let _: RefreshRequest = decode(value.refresh_request);
    let refresh: RefreshResponse = decode(value.refresh_response);
    assert_eq!(metadata.api_version, HOST_API);
    refresh.page.validate().unwrap();
}

#[test]
fn new_host_reads_old_plugin_responses_and_old_plugin_reads_new_host_requests() {
    let old = fixture(include_str!("fixtures/abi-old-minor.json"));
    let current = fixture(include_str!("fixtures/abi-current.json"));
    assert!(old.api_version.is_compatible_with(HOST_API));

    let old_metadata: MetadataResponse = decode(old.metadata_response);
    let old_config: ValidateConfigResponse = decode(old.validate_config_response);
    let old_refresh: RefreshResponse = decode(old.refresh_response);
    assert_eq!(old_metadata.api_version, ApiVersion { major: 1, minor: 0 });
    assert!(old_config.diagnostics.is_empty());
    old_refresh.page.validate().unwrap();

    let metadata_request: V1MetadataRequest = decode(current.metadata_request);
    let config_request: V1ValidateConfigRequest = decode(current.validate_config_request);
    let refresh_request: V1RefreshRequest = decode(current.refresh_request);
    assert_eq!(metadata_request.host_api_version, HOST_API);
    assert_eq!(metadata_request.host_version, "0.3.0");
    assert_eq!(config_request.config["enabled"], true);
    assert_eq!(refresh_request.generation, 7);
    assert_eq!(refresh_request.locale.as_deref(), Some("zh-CN"));
    assert_eq!(refresh_request.platform.as_deref(), Some("macos"));
    assert_eq!(refresh_request.now_unix_ms, 1_798_000_000_000);
    assert_eq!(refresh_request.config.unwrap()["enabled"], true);
}

#[test]
fn rejects_future_minor_and_different_major_before_other_calls() {
    for value in [
        fixture(include_str!("fixtures/abi-future-minor.json")),
        fixture(include_str!("fixtures/abi-different-major.json")),
    ] {
        assert!(!value.api_version.is_compatible_with(HOST_API));
    }
}

#[test]
fn rejects_unknown_fields_in_every_current_abi_message() {
    let value = fixture(include_str!("fixtures/abi-current.json"));
    assert_unknown_field_rejected::<MetadataRequest>(value.metadata_request);
    assert_unknown_field_rejected::<MetadataResponse>(value.metadata_response);
    assert_unknown_field_rejected::<ValidateConfigRequest>(value.validate_config_request);
    assert_unknown_field_rejected::<ValidateConfigResponse>(value.validate_config_response);
    assert_unknown_field_rejected::<RefreshRequest>(value.refresh_request);
    assert_unknown_field_rejected::<RefreshResponse>(value.refresh_response);
}
