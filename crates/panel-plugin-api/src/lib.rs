use panel_model::PageModel;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::{Host, Url};

pub const HOST_API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ApiVersion {
    pub fn is_compatible_with(self, host: Self) -> bool {
        self.major == host.major && self.minor <= host.minor
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: ApiVersion,
    pub minimum_host_version: String,
    pub entry: String,
    #[serde(default)]
    pub permissions: PluginPermissions,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.manifest_version != 1 {
            return Err(ManifestError::UnsupportedManifest(self.manifest_version));
        }
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(ManifestError::InvalidId);
        }
        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }
        if self.entry != "plugin.wasm" {
            return Err(ManifestError::InvalidEntry);
        }
        if !self.api_version.is_compatible_with(HOST_API_VERSION) {
            return Err(ManifestError::IncompatibleApi {
                plugin: self.api_version,
                host: HOST_API_VERSION,
            });
        }
        for origin in &self.permissions.network {
            validate_network_origin(origin)?;
        }
        Ok(())
    }
}

fn validate_network_origin(origin: &str) -> Result<(), ManifestError> {
    let url =
        Url::parse(origin).map_err(|_| ManifestError::InvalidNetworkOrigin(origin.to_owned()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ManifestError::InvalidNetworkOrigin(origin.to_owned()));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ManifestError::InvalidNetworkOrigin(origin.to_owned()));
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
        Err(ManifestError::UnsafeNetworkOrigin(origin.to_owned()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub cache: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub locale: String,
    pub platform: String,
    pub now_unix_ms: i64,
    pub config: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[error("plugin entry must be plugin.wasm")]
    InvalidEntry,
    #[error("plugin API {plugin:?} is incompatible with host API {host:?}")]
    IncompatibleApi {
        plugin: ApiVersion,
        host: ApiVersion,
    },
    #[error("network origin is malformed: {0}")]
    InvalidNetworkOrigin(String),
    #[error("network origin is not allowed: {0}")]
    UnsafeNetworkOrigin(String),
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
            entry: "plugin.wasm".into(),
            permissions: PluginPermissions::default(),
        }
    }

    #[test]
    fn accepts_current_api() {
        assert_eq!(manifest().validate(), Ok(()));
    }

    #[test]
    fn rejects_plaintext_remote_origins() {
        let mut value = manifest();
        value.permissions.network = vec!["http://example.com".into()];
        assert!(matches!(
            value.validate(),
            Err(ManifestError::UnsafeNetworkOrigin(_))
        ));
    }

    #[test]
    fn rejects_domains_disguised_as_localhost() {
        let mut value = manifest();
        value.permissions.network = vec!["http://localhost.evil.example".into()];
        assert!(matches!(
            value.validate(),
            Err(ManifestError::UnsafeNetworkOrigin(_))
        ));
    }

    #[test]
    fn rejects_credentials_and_paths_in_origins() {
        let mut credentials = manifest();
        credentials.permissions.network = vec!["https://user:secret@example.com".into()];
        assert!(matches!(
            credentials.validate(),
            Err(ManifestError::InvalidNetworkOrigin(_))
        ));

        let mut path = manifest();
        path.permissions.network = vec!["https://example.com/status".into()];
        assert!(matches!(
            path.validate(),
            Err(ManifestError::InvalidNetworkOrigin(_))
        ));
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
}
