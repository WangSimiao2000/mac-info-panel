use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub behavior: BehaviorConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    pub pages: Vec<PageConfig>,
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
}

impl AppConfig {
    pub fn from_jsonc(input: &str) -> Result<Self, ConfigError> {
        let json = strip_jsonc_comments(input)?;
        let config: Self = serde_json::from_str(&json)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        if self.pages.is_empty() {
            return Err(ConfigError::InvalidField {
                field: "pages",
                reason: "at least one page is required",
            });
        }
        if !(0..=200).contains(&self.display.padding) {
            return Err(ConfigError::InvalidField {
                field: "display.padding",
                reason: "must be between 0 and 200",
            });
        }
        if self.behavior.auto_rotate_seconds > 86_400 {
            return Err(ConfigError::InvalidField {
                field: "behavior.autoRotateSeconds",
                reason: "must not exceed 86400",
            });
        }
        if !(0.75..=2.0).contains(&self.appearance.scale) {
            return Err(ConfigError::InvalidField {
                field: "appearance.scale",
                reason: "must be between 0.75 and 2.0",
            });
        }

        let mut page_ids = HashSet::new();
        for page in &self.pages {
            validate_id("pages[].id", &page.id)?;
            validate_text("pages[].title", &page.title)?;
            if !page_ids.insert(page.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    field: "pages[].id",
                    id: page.id.clone(),
                });
            }
        }
        if !page_ids.contains(self.behavior.initial_page.as_str()) {
            return Err(ConfigError::InvalidField {
                field: "behavior.initialPage",
                reason: "must reference an existing page",
            });
        }

        let mut plugin_ids = HashSet::new();
        for plugin in &self.plugins {
            validate_id("plugins[].id", &plugin.id)?;
            if !plugin_ids.insert(plugin.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    field: "plugins[].id",
                    id: plugin.id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayConfig {
    pub name_contains: Option<String>,
    pub padding: i32,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            name_contains: None,
            padding: 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BehaviorConfig {
    pub initial_page: String,
    pub auto_rotate_seconds: u64,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            initial_page: "overview".into(),
            auto_rotate_seconds: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceConfig {
    pub scale: f32,
    pub reduce_motion: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            reduce_motion: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageConfig {
    pub id: String,
    pub title: String,
    pub kind: PageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageKind {
    BuiltIn,
    Plugin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginConfig {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("unterminated block comment at byte {0}")]
    UnterminatedBlockComment(usize),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid field {field}: {reason}")]
    InvalidField {
        field: &'static str,
        reason: &'static str,
    },
    #[error("duplicate value in {field}: {id}")]
    DuplicateId { field: &'static str, id: String },
}

pub fn strip_jsonc_comments(input: &str) -> Result<String, ConfigError> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if byte == b'"' {
            in_string = true;
            output.push(byte);
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            output.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                output.push(b' ');
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            output.extend_from_slice(b"  ");
            index += 2;
            let mut closed = false;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    output.extend_from_slice(b"  ");
                    index += 2;
                    closed = true;
                    break;
                }
                output.push(if bytes[index] == b'\n' { b'\n' } else { b' ' });
                index += 1;
            }
            if !closed {
                return Err(ConfigError::UnterminatedBlockComment(start));
            }
        } else {
            output.push(byte);
            index += 1;
        }
    }

    String::from_utf8(output).map_err(|_| ConfigError::InvalidField {
        field: "document",
        reason: "must be UTF-8",
    })
}

fn validate_id(field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ConfigError::InvalidField {
            field,
            reason: "contains invalid characters",
        });
    }
    Ok(())
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::InvalidField {
            field,
            reason: "cannot be empty",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
        // top-level comment
        "schemaVersion": 1,
        "pages": [{"id":"overview","title":"Overview","kind":"builtIn"}],
        "plugins": []
    }"#;

    #[test]
    fn parses_minimal_jsonc_config() {
        let config = AppConfig::from_jsonc(VALID).unwrap();
        assert_eq!(config.behavior.initial_page, "overview");
    }

    #[test]
    fn preserves_comment_markers_inside_strings() {
        let input = VALID.replace("Overview", "https://example.com/*status*/");
        let config = AppConfig::from_jsonc(&input).unwrap();
        assert_eq!(config.pages[0].title, "https://example.com/*status*/");
    }

    #[test]
    fn rejects_unknown_initial_page() {
        let input = VALID.replace(
            "\"plugins\": []",
            "\"behavior\": {\"initialPage\":\"missing\",\"autoRotateSeconds\":0},\"plugins\": []",
        );
        assert!(matches!(
            AppConfig::from_jsonc(&input),
            Err(ConfigError::InvalidField {
                field: "behavior.initialPage",
                ..
            })
        ));
    }

    #[test]
    fn rejects_duplicate_page_ids() {
        let input = VALID.replace(
            "{\"id\":\"overview\",\"title\":\"Overview\",\"kind\":\"builtIn\"}",
            "{\"id\":\"overview\",\"title\":\"A\",\"kind\":\"builtIn\"},{\"id\":\"overview\",\"title\":\"B\",\"kind\":\"builtIn\"}",
        );
        assert!(matches!(
            AppConfig::from_jsonc(&input),
            Err(ConfigError::DuplicateId { .. })
        ));
    }

    #[test]
    fn reports_unterminated_block_comments() {
        assert!(matches!(
            strip_jsonc_comments("{/* never closed"),
            Err(ConfigError::UnterminatedBlockComment(1))
        ));
    }
}
