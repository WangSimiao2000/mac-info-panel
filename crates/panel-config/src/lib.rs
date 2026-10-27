use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fs2::FileExt;
use panel_model::BuiltInPage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;
pub const CONFIG_FILE_NAME: &str = "config.jsonc";
pub const DEFAULT_CONFIG_JSONC: &str = include_str!("../default-config.jsonc");

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
        let mut deserializer = serde_json::Deserializer::from_str(&json);
        let config: Self =
            serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
                let field = normalize_serde_path(&error.path().to_string());
                ConfigError::Json {
                    field,
                    reason: error.inner().to_string(),
                }
            })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        if self.pages.is_empty() {
            return Err(invalid_field("pages", "at least one page is required"));
        }
        if !(0..=200).contains(&self.display.padding) {
            return Err(invalid_field(
                "display.padding",
                "must be between 0 and 200",
            ));
        }
        if self.behavior.auto_rotate_seconds > 86_400 {
            return Err(invalid_field(
                "behavior.autoRotateSeconds",
                "must not exceed 86400",
            ));
        }
        if !(0.75..=2.0).contains(&self.appearance.scale) {
            return Err(invalid_field(
                "appearance.scale",
                "must be between 0.75 and 2.0",
            ));
        }

        let mut plugin_ids = HashSet::new();
        let mut enabled_plugin_ids = HashSet::new();
        for (index, plugin) in self.plugins.iter().enumerate() {
            let field = format!("plugins[{index}].id");
            validate_id(field.clone(), &plugin.id)?;
            if !plugin_ids.insert(plugin.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    field,
                    id: plugin.id.clone(),
                });
            }
            if plugin.enabled {
                enabled_plugin_ids.insert(plugin.id.as_str());
            }
        }

        let mut page_ids = HashSet::new();
        for (index, page) in self.pages.iter().enumerate() {
            let id_field = format!("pages[{index}].id");
            validate_id(id_field.clone(), &page.id)?;
            validate_text(format!("pages[{index}].title"), &page.title)?;
            if !page_ids.insert(page.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    field: id_field,
                    id: page.id.clone(),
                });
            }
            match page.kind {
                PageKind::BuiltIn if BuiltInPage::from_id(&page.id).is_none() => {
                    return Err(invalid_field(
                        format!("pages[{index}].id"),
                        "is not a recognized built-in page identifier",
                    ));
                }
                PageKind::Plugin if !enabled_plugin_ids.contains(page.id.as_str()) => {
                    return Err(invalid_field(
                        format!("pages[{index}].id"),
                        "must reference an enabled plugin",
                    ));
                }
                PageKind::BuiltIn | PageKind::Plugin => {}
            }
        }
        if !page_ids.contains(self.behavior.initial_page.as_str()) {
            return Err(invalid_field(
                "behavior.initialPage",
                "must reference an existing page",
            ));
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
            initial_page: BuiltInPage::Overview.id().into(),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDiagnostic {
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("invalid field {field}: {reason}")]
    Json { field: String, reason: String },
    #[error("unterminated block comment at byte {0}")]
    UnterminatedBlockComment(usize),
    #[error("unsupported schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid field {field}: {reason}")]
    InvalidField { field: String, reason: String },
    #[error("duplicate value in {field}: {id}")]
    DuplicateId { field: String, id: String },
}

impl ConfigError {
    pub fn diagnostic(&self) -> ConfigDiagnostic {
        match self {
            Self::Json { field, reason } | Self::InvalidField { field, reason } => {
                ConfigDiagnostic {
                    field: field.clone(),
                    reason: reason.clone(),
                }
            }
            Self::UnterminatedBlockComment(offset) => ConfigDiagnostic {
                field: "document".into(),
                reason: format!("unterminated block comment at byte {offset}"),
            },
            Self::UnsupportedSchema(version) => ConfigDiagnostic {
                field: "schemaVersion".into(),
                reason: format!("unsupported schema version {version}"),
            },
            Self::DuplicateId { field, id } => ConfigDiagnostic {
                field: field.clone(),
                reason: format!("duplicate identifier {id}"),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigPlatform {
    MacOs,
    Windows,
    Linux,
}

pub fn config_path_for(
    platform: ConfigPlatform,
    home: Option<&Path>,
    app_data: Option<&Path>,
    xdg_config_home: Option<&Path>,
) -> Result<PathBuf, ConfigPathError> {
    match platform {
        ConfigPlatform::MacOs => home
            .map(|path| {
                path.join("Library")
                    .join("Application Support")
                    .join("Info Panel")
                    .join(CONFIG_FILE_NAME)
            })
            .ok_or(ConfigPathError::MissingBaseDirectory("HOME")),
        ConfigPlatform::Windows => app_data
            .map(|path| path.join("Info Panel").join(CONFIG_FILE_NAME))
            .ok_or(ConfigPathError::MissingBaseDirectory("APPDATA")),
        ConfigPlatform::Linux => {
            if let Some(path) = xdg_config_home.filter(|path| path.is_absolute()) {
                return Ok(path.join("info-panel").join(CONFIG_FILE_NAME));
            }
            home.map(|path| path.join(".config/info-panel").join(CONFIG_FILE_NAME))
                .ok_or(ConfigPathError::MissingBaseDirectory(
                    "XDG_CONFIG_HOME or HOME",
                ))
        }
    }
}

pub fn default_config_path() -> Result<PathBuf, ConfigPathError> {
    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME").map(PathBuf::from);
        config_path_for(ConfigPlatform::MacOs, home.as_deref(), None, None)
    }
    #[cfg(target_os = "windows")]
    {
        let app_data = env::var_os("APPDATA").map(PathBuf::from);
        config_path_for(ConfigPlatform::Windows, None, app_data.as_deref(), None)
    }
    #[cfg(target_os = "linux")]
    {
        let home = env::var_os("HOME").map(PathBuf::from);
        let xdg = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        config_path_for(ConfigPlatform::Linux, home.as_deref(), None, xdg.as_deref())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigPathError {
    #[error("cannot determine the Info Panel configuration directory: {0} is not set")]
    MissingBaseDirectory(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub config: AppConfig,
    pub created: bool,
}

#[derive(Debug, Error)]
pub enum ConfigBootstrapError {
    #[error(transparent)]
    Path(#[from] ConfigPathError),
    #[error("failed to {operation} Info Panel configuration at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid Info Panel configuration at {path}: {source}")]
    Invalid {
        path: PathBuf,
        #[source]
        source: ConfigError,
    },
}

impl ConfigBootstrapError {
    pub fn diagnostic(&self) -> Option<ConfigDiagnostic> {
        match self {
            Self::Invalid { source, .. } => Some(source.diagnostic()),
            Self::Path(_) | Self::Io { .. } => None,
        }
    }
}

pub fn load_or_create_default() -> Result<LoadedConfig, ConfigBootstrapError> {
    let path = default_config_path()?;
    load_or_create_at(&path)
}

pub fn load_or_create_at(path: &Path) -> Result<LoadedConfig, ConfigBootstrapError> {
    load_or_create_with(path, &OsAtomicWriter)
}

trait AtomicWriter {
    fn write_new(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
}

struct OsAtomicWriter;

impl AtomicWriter for OsAtomicWriter {
    fn write_new(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let (temporary_path, mut temporary_file) = create_temporary_file(path)?;
        let result = (|| {
            temporary_file.write_all(contents)?;
            temporary_file.sync_all()?;
            fs::rename(&temporary_path, path)?;
            sync_directory(parent)?;
            Ok(())
        })();
        if result.is_err() {
            drop(temporary_file);
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }
}

fn load_or_create_with(
    path: &Path,
    writer: &dyn AtomicWriter,
) -> Result<LoadedConfig, ConfigBootstrapError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io_error("create directory for", path, source))?;

    let lock_path = sibling_with_suffix(path, ".lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| io_error("open startup lock for", path, source))?;
    lock.lock_exclusive()
        .map_err(|source| io_error("lock", path, source))?;

    let result = match fs::metadata(path) {
        Ok(_) => load_existing(path, false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            writer
                .write_new(path, DEFAULT_CONFIG_JSONC.as_bytes())
                .map_err(|source| io_error("create", path, source))?;
            load_existing(path, true)
        }
        Err(source) => Err(io_error("inspect", path, source)),
    };

    FileExt::unlock(&lock).map_err(|source| io_error("unlock", path, source))?;
    result
}

fn load_existing(path: &Path, created: bool) -> Result<LoadedConfig, ConfigBootstrapError> {
    let input = fs::read_to_string(path).map_err(|source| io_error("read", path, source))?;
    let config = AppConfig::from_jsonc(&input).map_err(|source| ConfigBootstrapError::Invalid {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(LoadedConfig {
        path: path.to_path_buf(),
        config,
        created,
    })
}

fn create_temporary_file(path: &Path) -> io::Result<(PathBuf, File)> {
    for _ in 0..100 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let suffix = format!(".tmp.{}.{}", std::process::id(), sequence);
        let temporary_path = sibling_with_suffix(path, &suffix);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary configuration file",
    ))
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(CONFIG_FILE_NAME))
        .to_os_string();
    file_name.push(suffix);
    path.with_file_name(file_name)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> ConfigBootstrapError {
    ConfigBootstrapError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn normalize_serde_path(path: &str) -> String {
    let path = path.trim_start_matches('.');
    if path.is_empty() {
        "document".into()
    } else {
        path.into()
    }
}

fn invalid_field(field: impl Into<String>, reason: impl Into<String>) -> ConfigError {
    ConfigError::InvalidField {
        field: field.into(),
        reason: reason.into(),
    }
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

    String::from_utf8(output).map_err(|_| invalid_field("document", "must be UTF-8"))
}

fn validate_id(field: impl Into<String>, value: &str) -> Result<(), ConfigError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_field(field, "contains invalid characters"));
    }
    Ok(())
}

fn validate_text(field: impl Into<String>, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(invalid_field(field, "cannot be empty"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::tempdir;

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
    fn reports_deserialization_field_path() {
        let input = VALID.replace("\"overview\"", "42");
        let error = AppConfig::from_jsonc(&input).unwrap_err();
        assert_eq!(error.diagnostic().field, "pages[0].id");
    }

    #[test]
    fn rejects_unknown_initial_page() {
        let input = VALID.replace(
            "\"plugins\": []",
            "\"behavior\": {\"initialPage\":\"missing\",\"autoRotateSeconds\":0},\"plugins\": []",
        );
        assert!(matches!(
            AppConfig::from_jsonc(&input),
            Err(ConfigError::InvalidField { field, .. }) if field == "behavior.initialPage"
        ));
    }

    #[test]
    fn rejects_unknown_built_in_identifier() {
        let input = VALID.replace("overview", "invented");
        assert!(matches!(
            AppConfig::from_jsonc(&input),
            Err(ConfigError::InvalidField { field, .. }) if field == "pages[0].id"
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
            Err(ConfigError::DuplicateId { field, .. }) if field == "pages[1].id"
        ));
    }

    #[test]
    fn reports_unterminated_block_comments() {
        assert!(matches!(
            strip_jsonc_comments("{/* never closed"),
            Err(ConfigError::UnterminatedBlockComment(1))
        ));
    }

    #[test]
    fn defines_platform_specific_new_config_paths() {
        let home = Path::new("/home/person");
        assert_eq!(
            config_path_for(ConfigPlatform::MacOs, Some(home), None, None).unwrap(),
            home.join("Library/Application Support/Info Panel/config.jsonc")
        );
        assert_eq!(
            config_path_for(
                ConfigPlatform::Windows,
                None,
                Some(Path::new("C:/Users/person/AppData/Roaming")),
                None,
            )
            .unwrap(),
            Path::new("C:/Users/person/AppData/Roaming/Info Panel/config.jsonc")
        );
        assert_eq!(
            config_path_for(
                ConfigPlatform::Linux,
                Some(home),
                None,
                Some(Path::new("/custom/config")),
            )
            .unwrap(),
            Path::new("/custom/config/info-panel/config.jsonc")
        );
    }

    #[test]
    fn creates_default_configuration_on_first_run() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        let loaded = load_or_create_at(&path).unwrap();
        assert!(loaded.created);
        assert_eq!(loaded.config.schema_version, SCHEMA_VERSION);
        assert_eq!(fs::read_to_string(path).unwrap(), DEFAULT_CONFIG_JSONC);
    }

    #[test]
    fn loads_existing_valid_configuration_without_overwriting_it() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, VALID).unwrap();
        let loaded = load_or_create_at(&path).unwrap();
        assert!(!loaded.created);
        assert_eq!(fs::read_to_string(path).unwrap(), VALID);
    }

    #[test]
    fn refuses_invalid_existing_configuration_without_overwriting_it() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        let invalid = VALID.replace("overview", "missing");
        fs::write(&path, &invalid).unwrap();
        let error = load_or_create_at(&path).unwrap_err();
        assert_eq!(error.diagnostic().unwrap().field, "pages[0].id");
        assert_eq!(fs::read_to_string(path).unwrap(), invalid);
    }

    struct FailingWriter;

    impl AtomicWriter for FailingWriter {
        fn write_new(&self, _path: &Path, _contents: &[u8]) -> io::Result<()> {
            Err(io::Error::other("injected atomic write failure"))
        }
    }

    #[test]
    fn leaves_no_target_after_atomic_write_failure() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        assert!(load_or_create_with(&path, &FailingWriter).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn concurrent_startup_creates_once_and_loads_consistently() {
        const THREADS: usize = 8;
        let directory = tempdir().unwrap();
        let path = Arc::new(directory.path().join(CONFIG_FILE_NAME));
        let barrier = Arc::new(Barrier::new(THREADS));
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    load_or_create_at(&path).unwrap()
                })
            })
            .collect();
        let loaded: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(loaded.iter().filter(|value| value.created).count(), 1);
        assert!(
            loaded
                .iter()
                .all(|value| value.config.schema_version == SCHEMA_VERSION)
        );
        assert_eq!(fs::read_to_string(&*path).unwrap(), DEFAULT_CONFIG_JSONC);
    }
}
