use std::collections::BTreeSet;

use panel_model::PageModel;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::{Host, Url};

pub const HOST_API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };
pub const HOST_VERSION: &str = "0.1.0";
pub const METADATA_FUNCTION: &str = "metadata";
pub const VALIDATE_CONFIG_FUNCTION: &str = "validate_config";
pub const REFRESH_FUNCTION: &str = "refresh";

pub const MAX_MANIFEST_ID_BYTES: usize = 256;
pub const MAX_MANIFEST_NAME_BYTES: usize = 256;
pub const MAX_VERSION_BYTES: usize = 64;
pub const MAX_SOURCE_URL_BYTES: usize = 2_048;
pub const MAX_LICENSE_BYTES: usize = 128;
pub const MAX_MANIFEST_PAGES: usize = 64;
pub const MAX_PAGE_TITLE_BYTES: usize = 256;
pub const MAX_PAGE_ICON_BYTES: usize = 128;
pub const MAX_NETWORK_ORIGINS: usize = 32;
pub const MAX_NETWORK_ORIGIN_BYTES: usize = 2_048;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ApiVersion {
    #[must_use]
    pub fn is_compatible_with(self, host: Self) -> bool {
        self.major == host.major && self.minor <= host.minor
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginRuntimeKind {
    #[serde(rename = "wasm@1")]
    WasmV1,
    #[serde(rename = "lua@1")]
    LuaV1,
}

impl PluginRuntimeKind {
    #[must_use]
    pub const fn expected_entry(self) -> &'static str {
        match self {
            Self::WasmV1 => "plugin.wasm",
            Self::LuaV1 => "plugin.lua",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPageMetadata {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
}

impl PluginPageMetadata {
    fn validate(&self) -> Result<(), ManifestError> {
        validate_identifier(&self.id).map_err(|_| ManifestError::InvalidPageId)?;
        if self.title.trim().is_empty() {
            return Err(ManifestError::EmptyPageTitle);
        }
        if self.title.len() > MAX_PAGE_TITLE_BYTES {
            return Err(ManifestError::PageTitleTooLong);
        }
        if self.icon.as_ref().is_some_and(|icon| {
            icon.len() > MAX_PAGE_ICON_BYTES || validate_identifier(icon).is_err()
        }) {
            return Err(ManifestError::InvalidPageIcon);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: ApiVersion,
    pub minimum_host_version: String,
    pub runtime: PluginRuntimeKind,
    pub entry: String,
    pub pages: Vec<PluginPageMetadata>,
    #[serde(default)]
    pub permissions: PluginPermissions,
    pub source: String,
    pub license: String,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        self.validate_for_host(HOST_VERSION, HOST_API_VERSION)
    }

    pub fn validate_for_host(
        &self,
        host_version: &str,
        host_api_version: ApiVersion,
    ) -> Result<(), ManifestError> {
        if self.manifest_version != 1 {
            return Err(ManifestError::UnsupportedManifest(self.manifest_version));
        }
        validate_identifier(&self.id).map_err(|_| ManifestError::InvalidId)?;
        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }
        if self.name.len() > MAX_MANIFEST_NAME_BYTES {
            return Err(ManifestError::NameTooLong);
        }
        if self.version.len() > MAX_VERSION_BYTES
            || self.minimum_host_version.len() > MAX_VERSION_BYTES
        {
            return Err(ManifestError::VersionTooLong);
        }
        Version::parse(&self.version).map_err(|_| ManifestError::InvalidVersion("version"))?;
        let minimum_host = Version::parse(&self.minimum_host_version)
            .map_err(|_| ManifestError::InvalidVersion("minimumHostVersion"))?;
        let host = Version::parse(host_version)
            .map_err(|_| ManifestError::InvalidVersion("hostVersion"))?;
        if minimum_host > host {
            return Err(ManifestError::HostTooOld {
                minimum: minimum_host.to_string(),
                host: host.to_string(),
            });
        }
        if self.entry != self.runtime.expected_entry() {
            return Err(ManifestError::EntryRuntimeMismatch);
        }
        if !self.api_version.is_compatible_with(host_api_version) {
            return Err(ManifestError::IncompatibleApi {
                plugin: self.api_version,
                host: host_api_version,
            });
        }
        if self.pages.is_empty() {
            return Err(ManifestError::NoPages);
        }
        if self.pages.len() > MAX_MANIFEST_PAGES {
            return Err(ManifestError::TooManyPages);
        }
        let mut page_ids = BTreeSet::new();
        for page in &self.pages {
            page.validate()?;
            if !page_ids.insert(&page.id) {
                return Err(ManifestError::DuplicatePageId(page.id.clone()));
            }
        }
        self.permissions.validate()?;
        if self.source.len() > MAX_SOURCE_URL_BYTES {
            return Err(ManifestError::SourceTooLong);
        }
        validate_https_document_url("source", &self.source)?;
        if self.license.trim().is_empty() || self.license.len() > MAX_LICENSE_BYTES {
            return Err(ManifestError::InvalidLicense);
        }
        Ok(())
    }
}

fn validate_https_document_url(field: &'static str, value: &str) -> Result<(), ManifestError> {
    let url = Url::parse(value).map_err(|_| ManifestError::InvalidUrl(field))?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ManifestError::InvalidUrl(field));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > MAX_MANIFEST_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Err(())
    } else {
        Ok(())
    }
}

fn validate_network_origin(origin: &str) -> Result<(), ManifestError> {
    let url = Url::parse(origin).map_err(|_| ManifestError::InvalidNetworkOrigin)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ManifestError::InvalidNetworkOrigin);
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ManifestError::InvalidNetworkOrigin);
    }

    let allowed = match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Domain(host))) => host.eq_ignore_ascii_case("localhost"),
        ("http", Some(Host::Ipv4(address))) => address.is_loopback(),
        ("http", Some(Host::Ipv6(address))) => address.is_loopback(),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(ManifestError::UnsafeNetworkOrigin)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPermissions {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub cache: bool,
    #[serde(default)]
    pub config: bool,
    #[serde(default)]
    pub locale: bool,
    #[serde(default)]
    pub platform: bool,
}

impl PluginPermissions {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.network.len() > MAX_NETWORK_ORIGINS {
            return Err(ManifestError::TooManyNetworkOrigins);
        }
        let mut origins = BTreeSet::new();
        for origin in &self.network {
            if origin.len() > MAX_NETWORK_ORIGIN_BYTES {
                return Err(ManifestError::NetworkOriginTooLong);
            }
            validate_network_origin(origin)?;
            if !origins.insert(origin) {
                return Err(ManifestError::DuplicateNetworkOrigin);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn missing_from(&self, granted: &Self) -> PermissionDelta {
        let granted_network = granted.network.iter().collect::<BTreeSet<_>>();
        PermissionDelta {
            network: self
                .network
                .iter()
                .filter(|origin| !granted_network.contains(origin))
                .cloned()
                .collect(),
            cache: self.cache && !granted.cache,
            config: self.config && !granted.config,
            locale: self.locale && !granted.locale,
            platform: self.platform && !granted.platform,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDelta {
    pub network: Vec<String>,
    pub cache: bool,
    pub config: bool,
    pub locale: bool,
    pub platform: bool,
}

impl PermissionDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.network.is_empty() && !self.cache && !self.config && !self.locale && !self.platform
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageIdentity {
    pub id: String,
    pub version: String,
    pub entry: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionGrant {
    pub plugin_id: String,
    pub permissions: PluginPermissions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataRequest {
    pub host_api_version: ApiVersion,
    pub host_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataResponse {
    pub id: String,
    pub version: String,
    pub api_version: ApiVersion,
    pub runtime: PluginRuntimeKind,
    pub pages: Vec<PluginPageMetadata>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateConfigRequest {
    pub config: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateConfigResponse {
    pub valid: bool,
    #[serde(default)]
    pub diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshRequest {
    pub generation: u64,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    pub now_unix_ms: i64,
    #[serde(default)]
    pub config: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshResponse {
    pub page: PageModel,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("unsupported manifest version {0}")]
    UnsupportedManifest(u32),
    #[error("plugin id is invalid")]
    InvalidId,
    #[error("plugin name cannot be empty")]
    EmptyName,
    #[error("plugin name exceeds its byte limit")]
    NameTooLong,
    #[error("plugin version exceeds its byte limit")]
    VersionTooLong,
    #[error("{0} is not a valid semantic version")]
    InvalidVersion(&'static str),
    #[error("plugin requires host {minimum}, but current host is {host}")]
    HostTooOld { minimum: String, host: String },
    #[error("plugin entry does not match its runtime")]
    EntryRuntimeMismatch,
    #[error("plugin API {plugin:?} is incompatible with host API {host:?}")]
    IncompatibleApi {
        plugin: ApiVersion,
        host: ApiVersion,
    },
    #[error("plugin must declare at least one page")]
    NoPages,
    #[error("plugin declares too many pages")]
    TooManyPages,
    #[error("plugin page id is invalid")]
    InvalidPageId,
    #[error("plugin page title cannot be empty")]
    EmptyPageTitle,
    #[error("plugin page title is too long")]
    PageTitleTooLong,
    #[error("plugin page icon is invalid")]
    InvalidPageIcon,
    #[error("plugin page id is duplicated: {0}")]
    DuplicatePageId(String),
    #[error("plugin declares too many network origins")]
    TooManyNetworkOrigins,
    #[error("network origin exceeds its byte limit")]
    NetworkOriginTooLong,
    #[error("network origin is malformed")]
    InvalidNetworkOrigin,
    #[error("network origin is not allowed")]
    UnsafeNetworkOrigin,
    #[error("network origin is duplicated")]
    DuplicateNetworkOrigin,
    #[error("plugin source URL exceeds its byte limit")]
    SourceTooLong,
    #[error("{0} URL must be credential-free HTTPS")]
    InvalidUrl(&'static str),
    #[error("plugin license identifier is invalid")]
    InvalidLicense,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PluginManifest {
        PluginManifest {
            manifest_version: 1,
            id: "io.github.example.status".into(),
            name: "Example".into(),
            version: "0.1.0".into(),
            api_version: HOST_API_VERSION,
            minimum_host_version: "0.1.0".into(),
            runtime: PluginRuntimeKind::WasmV1,
            entry: "plugin.wasm".into(),
            pages: vec![PluginPageMetadata {
                id: "example.status".into(),
                title: "Example".into(),
                icon: None,
            }],
            permissions: PluginPermissions::default(),
            source: "https://example.com/source".into(),
            license: "MIT".into(),
        }
    }

    #[test]
    fn accepts_current_api_and_runtime() {
        assert_eq!(manifest().validate(), Ok(()));
    }

    #[test]
    fn runtime_discriminator_round_trips_as_protocol_string() {
        assert_eq!(
            serde_json::to_string(&PluginRuntimeKind::WasmV1).unwrap(),
            r#""wasm@1""#
        );
        assert_eq!(
            serde_json::from_str::<PluginRuntimeKind>(r#""lua@1""#).unwrap(),
            PluginRuntimeKind::LuaV1
        );
    }

    #[test]
    fn rejects_entry_runtime_mismatch_and_future_host() {
        let mut lua = manifest();
        lua.runtime = PluginRuntimeKind::LuaV1;
        assert_eq!(lua.validate(), Err(ManifestError::EntryRuntimeMismatch));

        let mut future = manifest();
        future.minimum_host_version = "2.0.0".into();
        assert!(matches!(
            future.validate(),
            Err(ManifestError::HostTooOld { .. })
        ));
    }

    #[test]
    fn rejects_plaintext_remote_origins_and_credentials() {
        for origin in [
            "http://example.com",
            "http://localhost.evil.example",
            "https://user:secret@example.com",
            "https://example.com/status",
        ] {
            let mut value = manifest();
            value.permissions.network = vec![origin.into()];
            let error = value.validate().unwrap_err();
            assert!(!error.to_string().contains("secret"));
        }
    }

    #[test]
    fn enforces_manifest_collection_and_field_limits_before_parsing() {
        let mut value = manifest();
        value.name = "n".repeat(MAX_MANIFEST_NAME_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::NameTooLong));

        let mut value = manifest();
        value.id = "i".repeat(MAX_MANIFEST_ID_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::InvalidId));

        let mut value = manifest();
        value.pages[0].title = "t".repeat(MAX_PAGE_TITLE_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::PageTitleTooLong));

        let mut value = manifest();
        value.pages[0].id = "p".repeat(MAX_MANIFEST_ID_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::InvalidPageId));

        let mut value = manifest();
        value.pages[0].icon = Some("i".repeat(MAX_PAGE_ICON_BYTES + 1));
        assert_eq!(value.validate(), Err(ManifestError::InvalidPageIcon));

        let mut value = manifest();
        value.version = "1".repeat(MAX_VERSION_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::VersionTooLong));

        let mut value = manifest();
        value.source = "s".repeat(MAX_SOURCE_URL_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::SourceTooLong));

        let mut value = manifest();
        value.license = "L".repeat(MAX_LICENSE_BYTES + 1);
        assert_eq!(value.validate(), Err(ManifestError::InvalidLicense));

        let mut value = manifest();
        value.pages = vec![value.pages[0].clone(); MAX_MANIFEST_PAGES + 1];
        assert_eq!(value.validate(), Err(ManifestError::TooManyPages));

        let mut value = manifest();
        value.permissions.network = vec!["https://example.com".into(); MAX_NETWORK_ORIGINS + 1];
        assert_eq!(value.validate(), Err(ManifestError::TooManyNetworkOrigins));

        let mut value = manifest();
        value.permissions.network = vec![format!(
            "https://{}.example.com",
            "a".repeat(MAX_NETWORK_ORIGIN_BYTES)
        )];
        assert_eq!(value.validate(), Err(ManifestError::NetworkOriginTooLong));
    }

    #[test]
    fn compatibility_accepts_older_minor_only_within_same_major() {
        assert!(
            ApiVersion { major: 1, minor: 0 }.is_compatible_with(ApiVersion { major: 1, minor: 2 })
        );
        assert!(
            !ApiVersion { major: 1, minor: 3 }
                .is_compatible_with(ApiVersion { major: 1, minor: 2 })
        );
        assert!(
            !ApiVersion { major: 2, minor: 0 }
                .is_compatible_with(ApiVersion { major: 1, minor: 2 })
        );
    }

    #[test]
    fn accepts_loopback_http_origins() {
        for origin in [
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            let mut value = manifest();
            value.permissions.network = vec![origin.into()];
            assert_eq!(value.validate(), Ok(()));
        }
    }

    #[test]
    fn computes_only_new_permissions_for_upgrade_confirmation() {
        let requested = PluginPermissions {
            network: vec!["https://one.example".into(), "https://two.example".into()],
            cache: true,
            config: true,
            locale: false,
            platform: true,
        };
        let granted = PluginPermissions {
            network: vec!["https://one.example".into()],
            cache: true,
            config: false,
            locale: false,
            platform: false,
        };
        assert_eq!(
            requested.missing_from(&granted),
            PermissionDelta {
                network: vec!["https://two.example".into()],
                config: true,
                platform: true,
                ..PermissionDelta::default()
            }
        );
    }

    #[test]
    fn rejects_unknown_protocol_fields() {
        let json = r#"{"valid":true,"diagnostics":[],"secret":"leak"}"#;
        assert!(serde_json::from_str::<ValidateConfigResponse>(json).is_err());
    }
}
