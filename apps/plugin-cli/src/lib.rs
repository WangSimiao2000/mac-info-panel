mod secure_fs;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
#[cfg(feature = "runtime")]
use std::time::Duration;

use crc32fast::Hasher as Crc32;
use flate2::Compression;
use flate2::write::DeflateEncoder;
use panel_plugin_api::{HOST_API_VERSION, HOST_VERSION, PluginManifest, PluginRuntimeKind};
#[cfg(feature = "runtime")]
use panel_plugin_api::{PackageIdentity, PermissionGrant, PluginPermissions};
#[cfg(feature = "runtime")]
use panel_plugin_host::{HostLimits, PluginHost, PluginPackage, RefreshContext, RefreshMode};
#[cfg(feature = "runtime")]
use panel_plugin_runtime_extism::{ExtismRuntime, LuaWasmRuntime, PluginRuntime, RuntimeLimits};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

pub const PACKAGE_EXTENSION: &str = "info-panel-plugin";
pub const DEVELOPMENT_WARNING_CODE: &str = "plugin.local-unsigned";
pub const LOCAL_RUNNER_WARNING_CODE: &str = "plugin.local-runner";
pub const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
pub const MAX_CONFIG_SCHEMA_BYTES: usize = 1024 * 1024;
pub const MAX_SCHEMA_NODES: usize = 4_096;
pub const MAX_SCHEMA_DEPTH: usize = 64;
pub const MAX_SCHEMA_PATTERNS: usize = 128;
pub const MAX_SCHEMA_REFERENCES: usize = 256;
pub const MAX_LUA_RUNNER_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SIGNATURE_BYTES: usize = 64 * 1024;
pub const MAX_ENTRIES: usize = 256;
pub const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TOTAL_UNCOMPRESSED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_PATH_BYTES: usize = 512;
pub const MAX_COMPRESSION_RATIO: u64 = 100;

const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
const CENTRAL_FILE_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const VERSION_NEEDED: u16 = 20;
const VERSION_MADE_BY_UNIX: u16 = 0x0314;
const UTF8_FLAG: u16 = 1 << 11;
const DEFLATE_METHOD: u16 = 8;
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;
const REGULAR_FILE_MODE: u32 = 0o100644;
const EOCD_SIZE: usize = 22;
const CENTRAL_HEADER_SIZE: usize = 46;
const LOCAL_HEADER_SIZE: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageLimits {
    pub max_archive_bytes: usize,
    pub max_entries: usize,
    pub max_entry_bytes: usize,
    pub max_total_uncompressed_bytes: usize,
    pub max_manifest_bytes: usize,
    pub max_path_bytes: usize,
    pub max_compression_ratio: u64,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: MAX_ARCHIVE_BYTES,
            max_entries: MAX_ENTRIES,
            max_entry_bytes: MAX_ENTRY_BYTES,
            max_total_uncompressed_bytes: MAX_TOTAL_UNCOMPRESSED_BYTES,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
            max_path_bytes: MAX_PATH_BYTES,
            max_compression_ratio: MAX_COMPRESSION_RATIO,
        }
    }
}

impl PackageLimits {
    fn validate(self) -> Result<Self, PackageError> {
        if self.max_archive_bytes == 0
            || self.max_entries == 0
            || self.max_entry_bytes == 0
            || self.max_total_uncompressed_bytes == 0
            || self.max_manifest_bytes == 0
            || self.max_manifest_bytes > self.max_entry_bytes
            || self.max_path_bytes == 0
            || self.max_compression_ratio == 0
        {
            return Err(PackageError::InvalidLimits);
        }
        Ok(self)
    }
}

pub trait SignatureProvider {
    fn verify(&self, package_sha256: &[u8; 32], signature: &[u8]) -> Result<(), SignatureError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoTrustRoots;

impl SignatureProvider for NoTrustRoots {
    fn verify(&self, _package_sha256: &[u8; 32], _signature: &[u8]) -> Result<(), SignatureError> {
        Err(SignatureError::TrustRootUnavailable)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum VerificationMode<'a> {
    Signed(&'a [u8]),
    AllowUnsignedLocal,
}

#[derive(Clone, Copy, Debug)]
pub enum RunnerVerificationMode<'a> {
    Signed(&'a [u8]),
    TrustedDigest([u8; 32]),
    AllowUnsignedLocal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistentWarning {
    pub code: String,
    pub message: String,
    pub canonical_path: String,
    pub sha256: String,
    pub plugin_id: String,
    pub plugin_version: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedPackage {
    manifest: PluginManifest,
    entries: BTreeMap<String, Vec<u8>>,
    sha256: [u8; 32],
    warning: Option<PersistentWarning>,
}

impl VerifiedPackage {
    #[must_use]
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    #[must_use]
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub fn sha256_hex(&self) -> String {
        hex_digest(&self.sha256)
    }

    #[must_use]
    pub fn warning(&self) -> Option<&PersistentWarning> {
        self.warning.as_ref()
    }

    #[must_use]
    pub fn entry_bytes(&self) -> &[u8] {
        self.entries
            .get(&self.manifest.entry)
            .expect("validated package always contains its entry")
    }

    pub fn stage_in_temp(&self) -> Result<StagedPackage, PackageError> {
        let directory = tempfile::tempdir().map_err(PackageError::Io)?;
        set_private_directory_permissions(directory.path())?;
        for (name, bytes) in &self.entries {
            let destination = directory.path().join(name);
            if let Some(parent) = destination.parent() {
                create_private_directories(directory.path(), parent)?;
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&destination).map_err(PackageError::Io)?;
            file.write_all(bytes).map_err(PackageError::Io)?;
            file.sync_all().map_err(PackageError::Io)?;
            set_private_file_permissions(&destination)?;
        }
        Ok(StagedPackage { directory })
    }
}

pub struct StagedPackage {
    directory: TempDir,
}

impl StagedPackage {
    #[must_use]
    pub fn path(&self) -> &Path {
        self.directory.path()
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedLuaRunner {
    bytes: Vec<u8>,
    sha256: [u8; 32],
    canonical_path: PathBuf,
    warning: Option<PersistentWarning>,
}

impl VerifiedLuaRunner {
    #[must_use]
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    #[must_use]
    pub fn warning(&self) -> Option<&PersistentWarning> {
        self.warning.as_ref()
    }
}

#[cfg(feature = "runtime")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginTestResult {
    pub plugin_id: String,
    pub page_id: String,
    pub page_title: String,
    pub warnings: Vec<PersistentWarning>,
}

pub fn read_verified_package(
    path: &Path,
    limits: PackageLimits,
    signature_provider: &impl SignatureProvider,
    mode: VerificationMode<'_>,
) -> Result<VerifiedPackage, PackageError> {
    require_package_extension(path)?;
    let limits = limits.validate()?;
    let input = secure_fs::read_bound_file_with_error(
        path,
        limits.max_archive_bytes,
        PackageError::ArchiveTooLarge,
    )?;
    verify_package_bytes_bound(
        &input.bytes,
        limits,
        signature_provider,
        mode,
        Some(&input.canonical_path),
    )
}

pub fn verify_package_bytes(
    bytes: &[u8],
    limits: PackageLimits,
    signature_provider: &impl SignatureProvider,
    mode: VerificationMode<'_>,
) -> Result<VerifiedPackage, PackageError> {
    verify_package_bytes_bound(bytes, limits, signature_provider, mode, None)
}

fn verify_package_bytes_bound(
    bytes: &[u8],
    limits: PackageLimits,
    signature_provider: &impl SignatureProvider,
    mode: VerificationMode<'_>,
    canonical_path: Option<&Path>,
) -> Result<VerifiedPackage, PackageError> {
    let limits = limits.validate()?;
    if bytes.len() > limits.max_archive_bytes {
        return Err(PackageError::ArchiveTooLarge);
    }
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    if let VerificationMode::Signed(signature) = mode {
        verify_signature(signature_provider, &sha256, signature)?;
    }
    let entries = parse_archive(bytes, limits)?;
    let manifest_bytes = entries
        .get("manifest.json")
        .ok_or_else(|| PackageError::MissingRequired("manifest.json".to_owned()))?;
    let manifest: PluginManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| PackageError::InvalidManifestJson)?;
    manifest.validate().map_err(PackageError::Manifest)?;
    validate_package_contents(&entries, &manifest)?;
    let warning = match mode {
        VerificationMode::Signed(_) => None,
        VerificationMode::AllowUnsignedLocal => {
            let path = canonical_path.ok_or(PackageError::LocalPackagePathRequired)?;
            Some(local_warning(
                DEVELOPMENT_WARNING_CODE,
                "Unsigned local plugin loaded in development mode",
                path,
                &sha256,
                &manifest,
            )?)
        }
    };
    Ok(VerifiedPackage {
        manifest,
        entries,
        sha256,
        warning,
    })
}

pub fn read_verified_lua_runner(
    path: &Path,
    package: &VerifiedPackage,
    signature_provider: &impl SignatureProvider,
    mode: RunnerVerificationMode<'_>,
) -> Result<VerifiedLuaRunner, PackageError> {
    let input = secure_fs::read_bound_file_with_error(
        path,
        MAX_LUA_RUNNER_BYTES,
        PackageError::LuaRunnerTooLarge,
    )?;
    if !input.bytes.starts_with(b"\0asm") {
        return Err(PackageError::InvalidLuaRunner);
    }
    let sha256: [u8; 32] = Sha256::digest(&input.bytes).into();
    let warning = match mode {
        RunnerVerificationMode::Signed(signature) => {
            verify_signature(signature_provider, &sha256, signature)?;
            None
        }
        RunnerVerificationMode::TrustedDigest(expected) => {
            if expected != sha256 {
                return Err(PackageError::UntrustedLuaRunner);
            }
            None
        }
        RunnerVerificationMode::AllowUnsignedLocal => Some(local_warning(
            LOCAL_RUNNER_WARNING_CODE,
            "Unverified local Lua-in-WASM runner loaded in development mode",
            &input.canonical_path,
            &sha256,
            &package.manifest,
        )?),
    };
    Ok(VerifiedLuaRunner {
        bytes: input.bytes,
        sha256,
        canonical_path: input.canonical_path,
        warning,
    })
}

pub fn read_signature_file(path: &Path) -> Result<Vec<u8>, PackageError> {
    secure_fs::read_bound_file_with_error(
        path,
        MAX_SIGNATURE_BYTES,
        PackageError::SignatureTooLarge,
    )
    .map(|input| input.bytes)
}

pub fn read_config_file(path: &Path) -> Result<Vec<u8>, PackageError> {
    secure_fs::read_bound_file_with_error(path, MAX_CONFIG_BYTES, PackageError::ConfigTooLarge)
        .map(|input| input.bytes)
}

pub fn pack_directory(
    source: &Path,
    output: &Path,
    entry_override: Option<&Path>,
    limits: PackageLimits,
) -> Result<[u8; 32], PackageError> {
    require_package_extension(output)?;
    let limits = limits.validate()?;
    let source = secure_fs::SourceTree::open(source)?;
    let output = source.prepare_output(output)?;

    let manifest_bytes = source.read_root_file("manifest.json", limits.max_manifest_bytes)?;
    let manifest: PluginManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| PackageError::InvalidManifestJson)?;
    manifest.validate().map_err(PackageError::Manifest)?;

    let mut entries = BTreeMap::new();
    entries.insert("manifest.json".to_owned(), manifest_bytes);
    for required in ["README.md", "LICENSE"] {
        let bytes = source.read_root_file(required, limits.max_entry_bytes)?;
        entries.insert(required.to_owned(), bytes);
    }
    let entry_bytes = match entry_override {
        Some(path) => {
            secure_fs::read_bound_file_with_error(
                path,
                limits.max_entry_bytes,
                PackageError::EntryTooLarge,
            )?
            .bytes
        }
        None => source.read_root_file(&manifest.entry, limits.max_entry_bytes)?,
    };
    entries.insert(manifest.entry.clone(), entry_bytes);
    if let Some(schema) =
        source.read_optional_root_file("config.schema.json", MAX_CONFIG_SCHEMA_BYTES)?
    {
        entries.insert("config.schema.json".to_owned(), schema);
    }
    let mut assets = Vec::new();
    source.collect_assets(
        &mut assets,
        limits.max_entries.saturating_sub(entries.len()),
        limits.max_entry_bytes,
    )?;
    for (name, bytes) in assets {
        if entries.insert(name.clone(), bytes).is_some() {
            return Err(PackageError::DuplicatePath(name));
        }
    }
    validate_package_contents(&entries, &manifest)?;
    let archive = build_archive(&entries, limits)?;
    let digest: [u8; 32] = Sha256::digest(&archive).into();
    output.write_atomic(&archive)?;
    Ok(digest)
}

#[cfg(feature = "runtime")]
pub fn test_wasm_package(
    package: &VerifiedPackage,
    config: &[u8],
    grants: &PluginPermissions,
) -> Result<PluginTestResult, PackageError> {
    if package.manifest.runtime != PluginRuntimeKind::WasmV1 {
        return Err(PackageError::RuntimeMismatch);
    }
    test_with_runtime(package, config, grants, ExtismRuntime, Vec::new())
}

#[cfg(feature = "runtime")]
pub fn test_lua_package(
    package: &VerifiedPackage,
    config: &[u8],
    runner: &VerifiedLuaRunner,
    grants: &PluginPermissions,
) -> Result<PluginTestResult, PackageError> {
    if package.manifest.runtime != PluginRuntimeKind::LuaV1 {
        return Err(PackageError::RuntimeMismatch);
    }
    let runtime = LuaWasmRuntime::new(runner.bytes().to_vec()).map_err(PackageError::Runtime)?;
    let warnings = runner.warning.clone().into_iter().collect();
    test_with_runtime(package, config, grants, runtime, warnings)
}

#[cfg(feature = "runtime")]
fn test_with_runtime<R: PluginRuntime>(
    package: &VerifiedPackage,
    config: &[u8],
    grants: &PluginPermissions,
    runtime: R,
    mut warnings: Vec<PersistentWarning>,
) -> Result<PluginTestResult, PackageError> {
    if config.len() > MAX_CONFIG_BYTES {
        return Err(PackageError::ConfigTooLarge);
    }
    validate_config_against_schema(package, config)?;
    let staged = package.stage_in_temp()?;
    let entry_path = staged.path().join(&package.manifest.entry);
    let entry = secure_fs::read_bound_file_with_error(
        &entry_path,
        MAX_ENTRY_BYTES,
        PackageError::EntryTooLarge,
    )?
    .bytes;
    let host = PluginHost::new(
        runtime,
        HostLimits {
            runtime: RuntimeLimits {
                max_host_calls: 0,
                ..RuntimeLimits::default()
            },
            ..HostLimits::default()
        },
    )
    .map_err(PackageError::Host)?;
    let grant = PermissionGrant {
        plugin_id: package.manifest.id.clone(),
        permissions: grants.clone(),
    };
    let plugin_package = PluginPackage {
        identity: PackageIdentity {
            id: package.manifest.id.clone(),
            version: package.manifest.version.clone(),
            entry: package.manifest.entry.clone(),
        },
        entry: package.manifest.entry.clone(),
        bytes: entry,
    };
    let mut loaded = host
        .load(package.manifest.clone(), plugin_package, &grant)
        .map_err(PackageError::Host)?;
    let config_result = loaded
        .validate_config_json(config)
        .map_err(PackageError::Host)?;
    if !config_result.valid {
        return Err(PackageError::PluginConfigRejected);
    }
    let result = loaded
        .refresh(
            RefreshContext {
                locale: "en-US".to_owned(),
                platform: std::env::consts::OS.to_owned(),
                now_unix_ms: 0,
            },
            Duration::ZERO,
            RefreshMode::Scheduled,
        )
        .map_err(PackageError::Host)?;
    if let Some(warning) = package.warning.clone() {
        warnings.insert(0, warning);
    }
    Ok(PluginTestResult {
        plugin_id: package.manifest.id.clone(),
        page_id: result.response.page.id.as_str().to_owned(),
        page_title: result.response.page.title,
        warnings,
    })
}

fn verify_signature(
    provider: &impl SignatureProvider,
    digest: &[u8; 32],
    signature: &[u8],
) -> Result<(), PackageError> {
    if signature.is_empty() {
        return Err(PackageError::Signature(SignatureError::Missing));
    }
    if signature.len() > MAX_SIGNATURE_BYTES {
        return Err(PackageError::SignatureTooLarge);
    }
    provider
        .verify(digest, signature)
        .map_err(PackageError::Signature)
}

fn local_warning(
    code: &str,
    message: &str,
    canonical_path: &Path,
    digest: &[u8; 32],
    manifest: &PluginManifest,
) -> Result<PersistentWarning, PackageError> {
    let canonical_path = canonical_path
        .to_str()
        .ok_or(PackageError::NonUtf8ProvenancePath)?;
    Ok(PersistentWarning {
        code: code.to_owned(),
        message: message.to_owned(),
        canonical_path: canonical_path.to_owned(),
        sha256: hex_digest(digest),
        plugin_id: manifest.id.clone(),
        plugin_version: manifest.version.clone(),
    })
}

#[cfg(feature = "runtime")]
fn validate_config_against_schema(
    package: &VerifiedPackage,
    config: &[u8],
) -> Result<(), PackageError> {
    let instance: serde_json::Value =
        serde_json::from_slice(config).map_err(|_| PackageError::InvalidConfigJson)?;
    let Some(schema_bytes) = package.entries.get("config.schema.json") else {
        return Ok(());
    };
    let schema: serde_json::Value =
        serde_json::from_slice(schema_bytes).map_err(|_| PackageError::InvalidConfigSchemaJson)?;
    analyze_schema(&schema)?;
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .offline()
        .should_validate_formats(false)
        .with_pattern_options(
            jsonschema::PatternOptions::regex()
                .size_limit(64 * 1024)
                .dfa_size_limit(64 * 1024),
        )
        .build(&schema)
        .map_err(|_| PackageError::InvalidConfigSchemaJson)?;
    if !validator.is_valid(&instance) {
        return Err(PackageError::ConfigSchemaRejected);
    }
    Ok(())
}

fn analyze_schema(schema: &serde_json::Value) -> Result<(), PackageError> {
    let mut stack = vec![(schema, String::new(), 0_usize)];
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    let mut references = Vec::<(String, String)>::new();
    let mut node_count = 0_usize;
    let mut pattern_count = 0_usize;

    while let Some((value, pointer, depth)) = stack.pop() {
        node_count = node_count
            .checked_add(1)
            .ok_or(PackageError::SchemaTooManyNodes)?;
        if node_count > MAX_SCHEMA_NODES {
            return Err(PackageError::SchemaTooManyNodes);
        }
        if depth > MAX_SCHEMA_DEPTH {
            return Err(PackageError::SchemaTooDeep);
        }
        edges.entry(pointer.clone()).or_default();
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("pattern") {
                    pattern_count = pattern_count
                        .checked_add(1)
                        .ok_or(PackageError::SchemaTooManyPatterns)?;
                }
                if let Some(patterns) = object.get("patternProperties") {
                    let patterns = patterns
                        .as_object()
                        .ok_or(PackageError::InvalidConfigSchemaJson)?;
                    pattern_count = pattern_count
                        .checked_add(patterns.len())
                        .ok_or(PackageError::SchemaTooManyPatterns)?;
                }
                if pattern_count > MAX_SCHEMA_PATTERNS {
                    return Err(PackageError::SchemaTooManyPatterns);
                }
                for (key, child) in object {
                    if matches!(key.as_str(), "$ref" | "$dynamicRef") {
                        let reference = child
                            .as_str()
                            .ok_or(PackageError::ExternalSchemaReference)?;
                        let target = parse_local_schema_reference(reference)?;
                        references.push((pointer.clone(), target));
                        if references.len() > MAX_SCHEMA_REFERENCES {
                            return Err(PackageError::SchemaTooManyReferences);
                        }
                    }
                    let child_pointer = join_json_pointer(&pointer, key);
                    edges
                        .entry(pointer.clone())
                        .or_default()
                        .insert(child_pointer.clone());
                    stack.push((child, child_pointer, depth + 1));
                }
            }
            serde_json::Value::Array(values) => {
                for (index, child) in values.iter().enumerate() {
                    let child_pointer = join_json_pointer(&pointer, &index.to_string());
                    edges
                        .entry(pointer.clone())
                        .or_default()
                        .insert(child_pointer.clone());
                    stack.push((child, child_pointer, depth + 1));
                }
            }
            _ => {}
        }
    }

    for (source, target) in references {
        if schema.pointer(&target).is_none() {
            return Err(PackageError::InvalidLocalSchemaReference);
        }
        edges.entry(source).or_default().insert(target);
    }
    reject_schema_graph_cycles(&edges)
}

fn parse_local_schema_reference(reference: &str) -> Result<String, PackageError> {
    let Some(pointer) = reference.strip_prefix('#') else {
        return Err(PackageError::ExternalSchemaReference);
    };
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(PackageError::InvalidLocalSchemaReference);
    }
    if pointer.contains('%') || !is_valid_json_pointer(pointer) {
        return Err(PackageError::InvalidLocalSchemaReference);
    }
    Ok(pointer.to_owned())
}

fn is_valid_json_pointer(pointer: &str) -> bool {
    if pointer.is_empty() {
        return true;
    }
    if !pointer.starts_with('/') {
        return false;
    }
    let bytes = pointer.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            if bytes
                .get(index + 1)
                .is_none_or(|escaped| !matches!(escaped, b'0' | b'1'))
            {
                return false;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    true
}

fn join_json_pointer(parent: &str, token: &str) -> String {
    let escaped = token.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

fn reject_schema_graph_cycles(
    edges: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), PackageError> {
    let mut indegree = edges
        .keys()
        .map(|node| (node.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for targets in edges.values() {
        for target in targets {
            let degree = indegree
                .get_mut(target)
                .ok_or(PackageError::InvalidLocalSchemaReference)?;
            *degree = degree
                .checked_add(1)
                .ok_or(PackageError::SchemaTooManyNodes)?;
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(node.clone()))
        .collect::<Vec<_>>();
    let mut removed = 0_usize;
    while let Some(node) = ready.pop() {
        removed += 1;
        if let Some(targets) = edges.get(&node) {
            for target in targets {
                let degree = indegree
                    .get_mut(target)
                    .expect("all graph targets are registered nodes");
                *degree -= 1;
                if *degree == 0 {
                    ready.push(target.clone());
                }
            }
        }
    }
    if removed == indegree.len() {
        Ok(())
    } else {
        Err(PackageError::RecursiveLocalSchemaReference)
    }
}

fn create_private_directories(root: &Path, destination: &Path) -> Result<(), PackageError> {
    let relative = destination
        .strip_prefix(root)
        .map_err(|_| PackageError::InvalidPath)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(PackageError::InvalidPath);
        };
        current.push(component);
        match fs::create_dir(&current) {
            Ok(()) => set_private_directory_permissions(&current)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(PackageError::Io(error)),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), PackageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(PackageError::Io)
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), PackageError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), PackageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(PackageError::Io)
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), PackageError> {
    Ok(())
}

pub fn create_plugin_scaffold(
    destination: &Path,
    id: &str,
    name: &str,
    source: &str,
    runtime: PluginRuntimeKind,
) -> Result<(), PackageError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(PackageError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(PackageError::Io(error)),
    }
    let manifest = PluginManifest {
        manifest_version: 1,
        id: id.to_owned(),
        name: name.to_owned(),
        version: "0.1.0".to_owned(),
        api_version: HOST_API_VERSION,
        minimum_host_version: HOST_VERSION.to_owned(),
        runtime,
        entry: runtime.expected_entry().to_owned(),
        pages: vec![panel_plugin_api::PluginPageMetadata {
            id: format!("{id}.status"),
            title: name.to_owned(),
            icon: Some("gauge".to_owned()),
        }],
        permissions: panel_plugin_api::PluginPermissions::default(),
        source: source.to_owned(),
        license: "MIT".to_owned(),
    };
    manifest.validate().map_err(PackageError::Manifest)?;
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(PackageError::OutputHasNoParent);
    }
    let temporary = tempfile::Builder::new()
        .prefix(".info-panel-new-")
        .tempdir_in(parent)
        .map_err(PackageError::Io)?;
    set_private_directory_permissions(temporary.path())?;
    let destination_root = temporary.path();
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|_| PackageError::InvalidManifestJson)?
        + "\n";
    write_new(
        &destination_root.join("manifest.json"),
        manifest_json.as_bytes(),
    )?;
    write_new(
        &destination_root.join("README.md"),
        format!(
            "# {name}\n\nAn Info Panel plugin using API {}.{}.\n",
            HOST_API_VERSION.major, HOST_API_VERSION.minor
        )
        .as_bytes(),
    )?;
    write_new(
        &destination_root.join("LICENSE"),
        b"MIT License\n\nCopyright (c) 2026 Info Panel plugin authors\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the \"Software\"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:\n\nThe above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.\n",
    )?;
    match runtime {
        PluginRuntimeKind::LuaV1 => write_new(
            &destination_root.join("plugin.lua"),
            lua_scaffold(id, name).as_bytes(),
        )?,
        PluginRuntimeKind::WasmV1 => {
            fs::create_dir(destination_root.join("src")).map_err(PackageError::Io)?;
            set_private_directory_permissions(&destination_root.join("src"))?;
            write_new(
                &destination_root.join("Cargo.toml"),
                wasm_cargo_scaffold(id).as_bytes(),
            )?;
            write_new(
                &destination_root.join("src/lib.rs"),
                wasm_source_scaffold(id, name).as_bytes(),
            )?;
        }
    }
    sync_scaffold_tree(destination_root)?;
    fs::rename(destination_root, destination).map_err(PackageError::Io)?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(PackageError::Io)?;
    Ok(())
}

fn sync_scaffold_tree(root: &Path) -> Result<(), PackageError> {
    for entry in fs::read_dir(root).map_err(PackageError::Io)? {
        let entry = entry.map_err(PackageError::Io)?;
        if entry.file_type().map_err(PackageError::Io)?.is_dir() {
            sync_scaffold_tree(&entry.path())?;
        } else {
            fs::File::open(entry.path())
                .and_then(|file| file.sync_all())
                .map_err(PackageError::Io)?;
        }
    }
    fs::File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(PackageError::Io)
}

fn parse_archive(
    bytes: &[u8],
    limits: PackageLimits,
) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    if bytes.len() < EOCD_SIZE {
        return Err(PackageError::MalformedArchive);
    }
    let eocd_offset = bytes.len() - EOCD_SIZE;
    if read_u32(bytes, eocd_offset)? != END_OF_CENTRAL_DIRECTORY {
        return Err(PackageError::MalformedArchive);
    }
    let disk = read_u16(bytes, eocd_offset + 4)?;
    let central_disk = read_u16(bytes, eocd_offset + 6)?;
    let entries_on_disk = read_u16(bytes, eocd_offset + 8)?;
    let entry_count = read_u16(bytes, eocd_offset + 10)?;
    let central_size = read_u32(bytes, eocd_offset + 12)? as usize;
    let central_offset = read_u32(bytes, eocd_offset + 16)? as usize;
    let comment_len = read_u16(bytes, eocd_offset + 20)?;
    if usize::from(entry_count) > limits.max_entries {
        return Err(PackageError::TooManyEntries);
    }
    if disk != 0
        || central_disk != 0
        || entries_on_disk != entry_count
        || comment_len != 0
        || central_offset.checked_add(central_size) != Some(eocd_offset)
    {
        return Err(PackageError::NonCanonicalArchive);
    }

    let mut records = Vec::with_capacity(usize::from(entry_count));
    let mut cursor = central_offset;
    let mut paths = BTreeSet::new();
    let mut folded_paths = BTreeSet::new();
    let mut total_size = 0_u64;
    for _ in 0..entry_count {
        if read_u32(bytes, cursor)? != CENTRAL_FILE_HEADER {
            return Err(PackageError::MalformedArchive);
        }
        let made_by = read_u16(bytes, cursor + 4)?;
        let needed = read_u16(bytes, cursor + 6)?;
        let flags = read_u16(bytes, cursor + 8)?;
        let method = read_u16(bytes, cursor + 10)?;
        let time = read_u16(bytes, cursor + 12)?;
        let date = read_u16(bytes, cursor + 14)?;
        let crc32 = read_u32(bytes, cursor + 16)?;
        let compressed_size = read_u32(bytes, cursor + 20)?;
        let uncompressed_size = read_u32(bytes, cursor + 24)?;
        let name_len = usize::from(read_u16(bytes, cursor + 28)?);
        let extra_len = usize::from(read_u16(bytes, cursor + 30)?);
        let file_comment_len = usize::from(read_u16(bytes, cursor + 32)?);
        let disk_start = read_u16(bytes, cursor + 34)?;
        let internal_attributes = read_u16(bytes, cursor + 36)?;
        let external_attributes = read_u32(bytes, cursor + 38)?;
        let local_offset = read_u32(bytes, cursor + 42)?;
        if made_by != VERSION_MADE_BY_UNIX
            || needed != VERSION_NEEDED
            || flags != UTF8_FLAG
            || time != DOS_TIME
            || date != DOS_DATE
            || extra_len != 0
            || file_comment_len != 0
            || disk_start != 0
            || internal_attributes != 0
            || external_attributes != REGULAR_FILE_MODE << 16
        {
            return Err(PackageError::NonCanonicalArchive);
        }
        if method != DEFLATE_METHOD {
            return Err(PackageError::UnsupportedCompression(method));
        }
        preflight_sizes(compressed_size, uncompressed_size, &mut total_size, limits)?;
        let name_start = cursor
            .checked_add(CENTRAL_HEADER_SIZE)
            .ok_or(PackageError::MalformedArchive)?;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or(PackageError::MalformedArchive)?;
        let name = bytes
            .get(name_start..name_end)
            .ok_or(PackageError::MalformedArchive)?;
        let name = validate_archive_path(name, limits.max_path_bytes)?;
        if name == "manifest.json" && uncompressed_size as usize > limits.max_manifest_bytes {
            return Err(PackageError::ManifestTooLarge);
        }
        if name == "config.schema.json" && uncompressed_size as usize > MAX_CONFIG_SCHEMA_BYTES {
            return Err(PackageError::ConfigSchemaTooLarge);
        }
        if !paths.insert(name.clone()) {
            return Err(PackageError::DuplicatePath(name));
        }
        let folded = name.to_ascii_lowercase();
        if !folded_paths.insert(folded) {
            return Err(PackageError::CaseCollidingPath(name));
        }
        records.push(CentralRecord {
            name,
            crc32,
            compressed_size,
            uncompressed_size,
            local_offset,
        });
        cursor = name_end;
    }
    if cursor != eocd_offset || records.len() != usize::from(entry_count) {
        return Err(PackageError::MalformedArchive);
    }
    if records.windows(2).any(|pair| pair[0].name >= pair[1].name) {
        return Err(PackageError::NonCanonicalOrdering);
    }
    validate_path_prefixes(records.iter().map(|record| record.name.as_str()))?;

    let mut expected_local_offset = 0_usize;
    let mut entries = BTreeMap::new();
    for record in records {
        let local_offset = record.local_offset as usize;
        if local_offset != expected_local_offset
            || read_u32(bytes, local_offset)? != LOCAL_FILE_HEADER
        {
            return Err(PackageError::NonCanonicalArchive);
        }
        if read_u16(bytes, local_offset + 4)? != VERSION_NEEDED
            || read_u16(bytes, local_offset + 6)? != UTF8_FLAG
            || read_u16(bytes, local_offset + 8)? != DEFLATE_METHOD
            || read_u16(bytes, local_offset + 10)? != DOS_TIME
            || read_u16(bytes, local_offset + 12)? != DOS_DATE
            || read_u32(bytes, local_offset + 14)? != record.crc32
            || read_u32(bytes, local_offset + 18)? != record.compressed_size
            || read_u32(bytes, local_offset + 22)? != record.uncompressed_size
        {
            return Err(PackageError::LocalHeaderMismatch(record.name));
        }
        let name_len = usize::from(read_u16(bytes, local_offset + 26)?);
        let extra_len = usize::from(read_u16(bytes, local_offset + 28)?);
        if extra_len != 0 {
            return Err(PackageError::NonCanonicalArchive);
        }
        let name_start = local_offset
            .checked_add(LOCAL_HEADER_SIZE)
            .ok_or(PackageError::MalformedArchive)?;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or(PackageError::MalformedArchive)?;
        if bytes.get(name_start..name_end) != Some(record.name.as_bytes()) {
            return Err(PackageError::LocalHeaderMismatch(record.name));
        }
        let data_end = name_end
            .checked_add(record.compressed_size as usize)
            .ok_or(PackageError::MalformedArchive)?;
        if data_end > central_offset {
            return Err(PackageError::MalformedArchive);
        }
        let compressed = bytes
            .get(name_end..data_end)
            .ok_or(PackageError::MalformedArchive)?;
        let contents = inflate_bounded(
            compressed,
            record.uncompressed_size as usize,
            limits.max_entry_bytes,
        )?;
        let mut crc = Crc32::new();
        crc.update(&contents);
        if crc.finalize() != record.crc32 {
            return Err(PackageError::CrcMismatch(record.name));
        }
        entries.insert(record.name, contents);
        expected_local_offset = data_end;
    }
    if expected_local_offset != central_offset {
        return Err(PackageError::HiddenOrOverlappingData);
    }
    Ok(entries)
}

#[derive(Debug)]
struct CentralRecord {
    name: String,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_offset: u32,
}

fn preflight_sizes(
    compressed: u32,
    uncompressed: u32,
    total: &mut u64,
    limits: PackageLimits,
) -> Result<(), PackageError> {
    if uncompressed as usize > limits.max_entry_bytes {
        return Err(PackageError::EntryTooLarge);
    }
    *total = total
        .checked_add(u64::from(uncompressed))
        .ok_or(PackageError::TotalUncompressedTooLarge)?;
    if *total > limits.max_total_uncompressed_bytes as u64 {
        return Err(PackageError::TotalUncompressedTooLarge);
    }
    if compressed == 0
        || u64::from(uncompressed)
            > u64::from(compressed).saturating_mul(limits.max_compression_ratio)
    {
        return Err(PackageError::CompressionRatioTooHigh);
    }
    Ok(())
}

fn inflate_bounded(
    compressed: &[u8],
    expected_size: usize,
    hard_limit: usize,
) -> Result<Vec<u8>, PackageError> {
    let allocation = expected_size.min(hard_limit);
    let mut output = Vec::with_capacity(allocation);
    let max_read = hard_limit.saturating_add(1) as u64;
    let mut decoder = flate2::bufread::DeflateDecoder::new(compressed);
    (&mut decoder)
        .take(max_read)
        .read_to_end(&mut output)
        .map_err(|_| PackageError::InvalidCompressedData)?;
    if output.len() > hard_limit {
        return Err(PackageError::EntryTooLarge);
    }
    if output.len() != expected_size {
        return Err(PackageError::UncompressedSizeMismatch);
    }
    if !decoder.into_inner().is_empty() {
        return Err(PackageError::TrailingCompressedData);
    }
    let canonical = deflate_canonical(&output)?;
    if canonical != compressed {
        return Err(PackageError::NonCanonicalCompression);
    }
    Ok(output)
}

fn deflate_canonical(bytes: &[u8]) -> Result<Vec<u8>, PackageError> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(9));
    encoder.write_all(bytes).map_err(PackageError::Io)?;
    encoder.finish().map_err(PackageError::Io)
}

fn validate_package_contents(
    entries: &BTreeMap<String, Vec<u8>>,
    manifest: &PluginManifest,
) -> Result<(), PackageError> {
    validate_path_prefixes(entries.keys().map(String::as_str))?;
    for required in [
        "manifest.json",
        "README.md",
        "LICENSE",
        manifest.entry.as_str(),
    ] {
        if !entries.contains_key(required) {
            return Err(PackageError::MissingRequired(required.to_owned()));
        }
    }
    if entries.get("README.md").is_some_and(Vec::is_empty)
        || entries.get("LICENSE").is_some_and(Vec::is_empty)
        || entries.get(&manifest.entry).is_some_and(Vec::is_empty)
    {
        return Err(PackageError::EmptyRequiredFile);
    }
    for (name, bytes) in entries {
        let allowed = matches!(
            name.as_str(),
            "manifest.json" | "README.md" | "LICENSE" | "config.schema.json"
        ) || name == &manifest.entry
            || name.starts_with("assets/");
        if !allowed {
            return Err(PackageError::UnexpectedEntry(name.clone()));
        }
        if name != &manifest.entry && is_executable_payload(name, bytes) {
            return Err(PackageError::ExtraExecutablePayload(name.clone()));
        }
        if name.starts_with("assets/") {
            validate_asset(name, bytes)?;
        }
    }
    let opposite_entry = match manifest.runtime {
        PluginRuntimeKind::WasmV1 => "plugin.lua",
        PluginRuntimeKind::LuaV1 => "plugin.wasm",
    };
    if entries.contains_key(opposite_entry) {
        return Err(PackageError::EntryRuntimeMismatch);
    }
    let entry = entries
        .get(&manifest.entry)
        .ok_or_else(|| PackageError::MissingRequired("plugin entry".to_owned()))?;
    match manifest.runtime {
        PluginRuntimeKind::WasmV1 if !entry.starts_with(b"\0asm") => {
            return Err(PackageError::InvalidWasmEntry);
        }
        PluginRuntimeKind::LuaV1 if std::str::from_utf8(entry).is_err() => {
            return Err(PackageError::InvalidLuaEntry);
        }
        _ => {}
    }
    if let Some(schema) = entries.get("config.schema.json") {
        let value: serde_json::Value =
            serde_json::from_slice(schema).map_err(|_| PackageError::InvalidConfigSchemaJson)?;
        if !value.is_object() {
            return Err(PackageError::InvalidConfigSchemaJson);
        }
        analyze_schema(&value)?;
    }
    Ok(())
}

fn is_executable_payload(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    let executable_extension = [
        ".wasm", ".lua", ".exe", ".dll", ".dylib", ".so", ".com", ".bat", ".cmd", ".ps1", ".sh",
        ".py", ".js",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension));
    executable_extension
        || bytes.starts_with(b"\0asm")
        || bytes.starts_with(b"#!")
        || bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(b"MZ")
        || matches!(
            bytes.get(..4),
            Some(
                [0xfe, 0xed, 0xfa, 0xce]
                    | [0xfe, 0xed, 0xfa, 0xcf]
                    | [0xce, 0xfa, 0xed, 0xfe]
                    | [0xcf, 0xfa, 0xed, 0xfe]
                    | [0xca, 0xfe, 0xba, 0xbe]
                    | [0xbe, 0xba, 0xfe, 0xca]
                    | [0xca, 0xfe, 0xba, 0xbf]
                    | [0xbf, 0xba, 0xfe, 0xca]
            )
        )
}

fn validate_asset(name: &str, bytes: &[u8]) -> Result<(), PackageError> {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .ok_or_else(|| PackageError::UnsupportedAsset(name.to_owned()))?;
    let valid = match extension.to_ascii_lowercase().as_str() {
        "json" => serde_json::from_slice::<serde_json::Value>(bytes).is_ok(),
        "txt" | "md" | "csv" => std::str::from_utf8(bytes).is_ok(),
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12).is_some_and(|tag| tag == b"WEBP"),
        "bmp" => bytes.starts_with(b"BM"),
        "ico" => bytes.starts_with(b"\0\0\x01\0"),
        "avif" => {
            bytes.get(4..8).is_some_and(|tag| tag == b"ftyp")
                && bytes
                    .get(8..12)
                    .is_some_and(|brand| matches!(brand, b"avif" | b"avis" | b"mif1"))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(PackageError::InvalidAsset(name.to_owned()))
    }
}

fn validate_archive_path(bytes: &[u8], max_path_bytes: usize) -> Result<String, PackageError> {
    if bytes.is_empty() || bytes.len() > max_path_bytes || bytes.contains(&0) {
        return Err(PackageError::InvalidPath);
    }
    let name = std::str::from_utf8(bytes).map_err(|_| PackageError::InvalidPath)?;
    if name.nfc().collect::<String>() != name {
        return Err(PackageError::NonCanonicalUnicodePath);
    }
    if name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.ends_with('/')
        || name.contains(':')
    {
        return Err(PackageError::InvalidPath);
    }
    for segment in name.split('/') {
        if !valid_portable_segment(segment) {
            return Err(PackageError::InvalidPath);
        }
    }
    Ok(name.to_owned())
}

fn valid_portable_segment(segment: &str) -> bool {
    if segment.is_empty()
        || matches!(segment, "." | "..")
        || segment.ends_with(['.', ' '])
        || !segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return false;
    }
    let basename = segment
        .split_once('.')
        .map_or(segment, |(basename, _)| basename)
        .to_ascii_uppercase();
    !matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !is_numbered_windows_device(&basename, "COM")
        && !is_numbered_windows_device(&basename, "LPT")
}

fn is_numbered_windows_device(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

fn validate_path_prefixes<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Result<(), PackageError> {
    let mut folded_paths = BTreeMap::new();
    for path in paths {
        let folded = path.to_ascii_lowercase();
        if folded_paths.insert(folded, path).is_some() {
            return Err(PackageError::CaseCollidingPath(path.to_owned()));
        }
    }
    for (folded, original) in &folded_paths {
        let mut offset = 0;
        while let Some(relative) = folded[offset..].find('/') {
            offset += relative;
            let prefix = &folded[..offset];
            if let Some(prefix_original) = folded_paths.get(prefix) {
                return Err(PackageError::PathPrefixConflict {
                    file: (*prefix_original).to_owned(),
                    descendant: (*original).to_owned(),
                });
            }
            offset += 1;
        }
    }
    Ok(())
}

fn build_archive(
    entries: &BTreeMap<String, Vec<u8>>,
    limits: PackageLimits,
) -> Result<Vec<u8>, PackageError> {
    if entries.len() > limits.max_entries || entries.len() > usize::from(u16::MAX) {
        return Err(PackageError::TooManyEntries);
    }
    validate_path_prefixes(entries.keys().map(String::as_str))?;
    let mut local = Vec::new();
    let mut records = Vec::with_capacity(entries.len());
    let mut total = 0_u64;
    for (name, bytes) in entries {
        validate_archive_path(name.as_bytes(), limits.max_path_bytes)?;
        if bytes.len() > limits.max_entry_bytes {
            return Err(PackageError::EntryTooLarge);
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(PackageError::TotalUncompressedTooLarge)?;
        if total > limits.max_total_uncompressed_bytes as u64 {
            return Err(PackageError::TotalUncompressedTooLarge);
        }
        let compressed = deflate_canonical(bytes)?;
        if compressed.is_empty()
            || bytes.len() as u64
                > (compressed.len() as u64).saturating_mul(limits.max_compression_ratio)
        {
            return Err(PackageError::CompressionRatioTooHigh);
        }
        let compressed_size =
            u32::try_from(compressed.len()).map_err(|_| PackageError::EntryTooLarge)?;
        let uncompressed_size =
            u32::try_from(bytes.len()).map_err(|_| PackageError::EntryTooLarge)?;
        let local_offset = u32::try_from(local.len()).map_err(|_| PackageError::ArchiveTooLarge)?;
        let name_len = u16::try_from(name.len()).map_err(|_| PackageError::InvalidPath)?;
        let mut crc = Crc32::new();
        crc.update(bytes);
        let crc32 = crc.finalize();
        push_u32(&mut local, LOCAL_FILE_HEADER);
        push_u16(&mut local, VERSION_NEEDED);
        push_u16(&mut local, UTF8_FLAG);
        push_u16(&mut local, DEFLATE_METHOD);
        push_u16(&mut local, DOS_TIME);
        push_u16(&mut local, DOS_DATE);
        push_u32(&mut local, crc32);
        push_u32(&mut local, compressed_size);
        push_u32(&mut local, uncompressed_size);
        push_u16(&mut local, name_len);
        push_u16(&mut local, 0);
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(&compressed);
        records.push(CentralRecord {
            name: name.clone(),
            crc32,
            compressed_size,
            uncompressed_size,
            local_offset,
        });
    }
    let central_offset = u32::try_from(local.len()).map_err(|_| PackageError::ArchiveTooLarge)?;
    let mut central = Vec::new();
    for record in &records {
        push_u32(&mut central, CENTRAL_FILE_HEADER);
        push_u16(&mut central, VERSION_MADE_BY_UNIX);
        push_u16(&mut central, VERSION_NEEDED);
        push_u16(&mut central, UTF8_FLAG);
        push_u16(&mut central, DEFLATE_METHOD);
        push_u16(&mut central, DOS_TIME);
        push_u16(&mut central, DOS_DATE);
        push_u32(&mut central, record.crc32);
        push_u32(&mut central, record.compressed_size);
        push_u32(&mut central, record.uncompressed_size);
        push_u16(&mut central, record.name.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, REGULAR_FILE_MODE << 16);
        push_u32(&mut central, record.local_offset);
        central.extend_from_slice(record.name.as_bytes());
    }
    let central_size = u32::try_from(central.len()).map_err(|_| PackageError::ArchiveTooLarge)?;
    local.extend_from_slice(&central);
    push_u32(&mut local, END_OF_CENTRAL_DIRECTORY);
    push_u16(&mut local, 0);
    push_u16(&mut local, 0);
    push_u16(&mut local, records.len() as u16);
    push_u16(&mut local, records.len() as u16);
    push_u32(&mut local, central_size);
    push_u32(&mut local, central_offset);
    push_u16(&mut local, 0);
    if local.len() > limits.max_archive_bytes {
        return Err(PackageError::ArchiveTooLarge);
    }
    Ok(local)
}

fn require_package_extension(path: &Path) -> Result<(), PackageError> {
    if path.extension().and_then(|value| value.to_str()) != Some(PACKAGE_EXTENSION) {
        return Err(PackageError::InvalidPackageExtension);
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), PackageError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(PackageError::Io)?;
    file.write_all(bytes).map_err(PackageError::Io)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, PackageError> {
    let value: [u8; 2] = bytes
        .get(
            offset
                ..offset
                    .checked_add(2)
                    .ok_or(PackageError::MalformedArchive)?,
        )
        .ok_or(PackageError::MalformedArchive)?
        .try_into()
        .map_err(|_| PackageError::MalformedArchive)?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, PackageError> {
    let value: [u8; 4] = bytes
        .get(
            offset
                ..offset
                    .checked_add(4)
                    .ok_or(PackageError::MalformedArchive)?,
        )
        .ok_or(PackageError::MalformedArchive)?
        .try_into()
        .map_err(|_| PackageError::MalformedArchive)?;
    Ok(u32::from_le_bytes(value))
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn lua_scaffold(id: &str, name: &str) -> String {
    let id_literal = serde_json::to_string(id).expect("validated plugin id serializes");
    let page_id_literal =
        serde_json::to_string(&format!("{id}.status")).expect("validated page id serializes");
    let name_literal = serde_json::to_string(name).expect("validated plugin name serializes");
    format!(
        "function metadata(request)\n    return {{\n        id = {id_literal},\n        version = \"0.1.0\",\n        apiVersion = {{ major = 1, minor = 0 }},\n        runtime = \"lua@1\",\n        pages = {{{{ id = {page_id_literal}, title = {name_literal}, icon = \"gauge\" }}}},\n    }}\nend\n\nfunction validate_config(request)\n    return {{ valid = true }}\nend\n\nfunction refresh(request)\n    return {{\n        page = {{\n            schemaVersion = 1,\n            id = {page_id_literal},\n            title = {name_literal},\n            icon = \"gauge\",\n            sections = {{{{\n                title = \"Status\",\n                widgets = {{{{ kind = \"status\", label = \"Plugin\", value = \"Ready\", tone = \"success\" }}}},\n            }}}},\n            updatedAtUnixMs = request.nowUnixMs,\n        }},\n    }}\nend\n"
    )
}

fn wasm_cargo_scaffold(id: &str) -> String {
    let package = id.replace(['.', '_'], "-");
    format!(
        "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.92\"\nlicense = \"MIT\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nextism-pdk = {{ version = \"1.4.1\", default-features = false }}\nserde_json = \"1\"\n\n[workspace]\n"
    )
}

fn wasm_source_scaffold(id: &str, name: &str) -> String {
    let id_literal = format!("{id:?}");
    let page_id_literal = format!("{:?}", format!("{id}.status"));
    let name_literal = format!("{name:?}");
    format!(
        "use extism_pdk::*;\n\n#[plugin_fn]\npub fn metadata(_input: String) -> FnResult<String> {{\n    Ok(serde_json::json!({{\n        \"id\": {id_literal},\n        \"version\": \"0.1.0\",\n        \"apiVersion\": {{ \"major\": 1, \"minor\": 0 }},\n        \"runtime\": \"wasm@1\",\n        \"pages\": [{{ \"id\": {page_id_literal}, \"title\": {name_literal}, \"icon\": \"gauge\" }}]\n    }}).to_string())\n}}\n\n#[plugin_fn]\npub fn validate_config(_input: String) -> FnResult<String> {{\n    Ok(serde_json::json!({{ \"valid\": true }}).to_string())\n}}\n\n#[plugin_fn]\npub fn refresh(_input: String) -> FnResult<String> {{\n    Ok(serde_json::json!({{\n        \"page\": {{\n            \"schemaVersion\": 1,\n            \"id\": {page_id_literal},\n            \"title\": {name_literal},\n            \"icon\": \"gauge\",\n            \"sections\": [{{\n                \"title\": \"Status\",\n                \"widgets\": [{{\n                    \"kind\": \"status\",\n                    \"label\": \"Plugin\",\n                    \"value\": \"Ready\",\n                    \"tone\": \"success\"\n                }}]\n            }}]\n        }}\n    }}).to_string())\n}}\n"
    )
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("plugin signature is required")]
    Missing,
    #[error("no plugin signature trust root is configured")]
    TrustRootUnavailable,
    #[error("plugin signature is invalid")]
    Invalid,
}

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("package limits are invalid")]
    InvalidLimits,
    #[error("package path must end in .info-panel-plugin")]
    InvalidPackageExtension,
    #[error("archive is not a regular file")]
    ArchiveNotRegularFile,
    #[error("archive exceeds its byte limit")]
    ArchiveTooLarge,
    #[error("archive changed while being read or exceeds its byte limit")]
    ArchiveChangedOrTooLarge,
    #[error("local development verification requires a handle-bound package path")]
    LocalPackagePathRequired,
    #[error("Lua runner exceeds its byte limit")]
    LuaRunnerTooLarge,
    #[error("Lua runner is not a WebAssembly module")]
    InvalidLuaRunner,
    #[error("Lua runner digest is not trusted")]
    UntrustedLuaRunner,
    #[error("signature exceeds its byte limit")]
    SignatureTooLarge,
    #[error("configuration exceeds its byte limit")]
    ConfigTooLarge,
    #[error("configuration JSON is invalid")]
    InvalidConfigJson,
    #[error("configuration does not satisfy config.schema.json")]
    ConfigSchemaRejected,
    #[error("external JSON Schema references are forbidden")]
    ExternalSchemaReference,
    #[error("JSON Schema contains too many nodes")]
    SchemaTooManyNodes,
    #[error("JSON Schema nesting exceeds its depth limit")]
    SchemaTooDeep,
    #[error("JSON Schema contains too many regular expressions")]
    SchemaTooManyPatterns,
    #[error("JSON Schema contains too many local references")]
    SchemaTooManyReferences,
    #[error("JSON Schema local reference is invalid or unresolved")]
    InvalidLocalSchemaReference,
    #[error("recursive or cyclic local JSON Schema references are forbidden")]
    RecursiveLocalSchemaReference,
    #[error("archive is malformed")]
    MalformedArchive,
    #[error("archive metadata is not canonical")]
    NonCanonicalArchive,
    #[error("archive entries are not in canonical order")]
    NonCanonicalOrdering,
    #[error("archive contains hidden, overlapping, or unindexed data")]
    HiddenOrOverlappingData,
    #[error("ZIP compression method {0} is unsupported")]
    UnsupportedCompression(u16),
    #[error("archive entry path is invalid")]
    InvalidPath,
    #[error("archive path is not NFC-normalized")]
    NonCanonicalUnicodePath,
    #[error("archive contains duplicate path {0}")]
    DuplicatePath(String),
    #[error("archive contains case-colliding path {0}")]
    CaseCollidingPath(String),
    #[error("archive contains too many entries")]
    TooManyEntries,
    #[error("archive entry exceeds its uncompressed byte limit")]
    EntryTooLarge,
    #[error("archive total uncompressed bytes exceed the limit")]
    TotalUncompressedTooLarge,
    #[error("archive compression ratio exceeds the limit")]
    CompressionRatioTooHigh,
    #[error("manifest.json exceeds its byte limit")]
    ManifestTooLarge,
    #[error("config.schema.json exceeds its byte limit")]
    ConfigSchemaTooLarge,
    #[error("compressed entry data is invalid")]
    InvalidCompressedData,
    #[error("compressed entry contains trailing data")]
    TrailingCompressedData,
    #[error("entry compression is not the canonical Info Panel encoding")]
    NonCanonicalCompression,
    #[error("entry uncompressed size does not match metadata")]
    UncompressedSizeMismatch,
    #[error("entry CRC does not match metadata: {0}")]
    CrcMismatch(String),
    #[error("local ZIP header does not match central metadata: {0}")]
    LocalHeaderMismatch(String),
    #[error("required package entry is missing: {0}")]
    MissingRequired(String),
    #[error("required package entry is empty")]
    EmptyRequiredFile,
    #[error("package contains unexpected entry: {0}")]
    UnexpectedEntry(String),
    #[error("package contains an extra executable payload: {0}")]
    ExtraExecutablePayload(String),
    #[error("asset extension is not allowed: {0}")]
    UnsupportedAsset(String),
    #[error("asset content does not match its allowed media/data type: {0}")]
    InvalidAsset(String),
    #[error("package path {file} conflicts with descendant {descendant}")]
    PathPrefixConflict { file: String, descendant: String },
    #[error("manifest runtime and packaged entry do not match")]
    EntryRuntimeMismatch,
    #[error("WASM entry does not have a WebAssembly header")]
    InvalidWasmEntry,
    #[error("Lua entry is not valid UTF-8")]
    InvalidLuaEntry,
    #[error("manifest.json is invalid")]
    InvalidManifestJson,
    #[error("config.schema.json is invalid")]
    InvalidConfigSchemaJson,
    #[error("source is not a directory")]
    SourceNotDirectory,
    #[error("asset tree is not a directory")]
    AssetTreeNotDirectory,
    #[error("symbolic and hard links are not allowed")]
    LinksNotAllowed,
    #[error("special files are not allowed")]
    SpecialFileNotAllowed,
    #[error("source file changed while being read or exceeds its byte limit")]
    FileChangedOrTooLarge,
    #[error("output path is invalid")]
    InvalidOutputPath,
    #[error("output path is inside the source directory")]
    OutputInsideSource,
    #[error("output ancestry could not be proven outside the source directory")]
    OutputAncestryUnverifiable,
    #[error("output already exists")]
    OutputExists,
    #[error("mount points are not allowed in the package source tree")]
    MountPointNotAllowed,
    #[error("could not allocate a unique temporary output name")]
    TemporaryNameExhausted,
    #[error("the canonical provenance path no longer names the opened file")]
    ProvenancePathChanged,
    #[error("canonical provenance paths must be valid UTF-8")]
    NonUtf8ProvenancePath,
    #[error("output path has no parent directory")]
    OutputHasNoParent,
    #[error("destination already exists")]
    DestinationExists,
    #[error("selected test runtime does not match the manifest")]
    RuntimeMismatch,
    #[error("plugin rejected its test configuration")]
    PluginConfigRejected,
    #[error(transparent)]
    Signature(#[from] SignatureError),
    #[error(transparent)]
    Manifest(#[from] panel_plugin_api::ManifestError),
    #[cfg(feature = "runtime")]
    #[error(transparent)]
    Runtime(#[from] panel_plugin_runtime_extism::RuntimeError),
    #[cfg(feature = "runtime")]
    #[error(transparent)]
    Host(#[from] panel_plugin_host::HostError),
    #[error("filesystem operation failed")]
    Io(#[source] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ExactSignature;

    impl SignatureProvider for ExactSignature {
        fn verify(
            &self,
            package_sha256: &[u8; 32],
            signature: &[u8],
        ) -> Result<(), SignatureError> {
            if signature == package_sha256 {
                Ok(())
            } else {
                Err(SignatureError::Invalid)
            }
        }
    }

    fn manifest(runtime: PluginRuntimeKind) -> PluginManifest {
        PluginManifest {
            manifest_version: 1,
            id: "io.github.example.test".to_owned(),
            name: "Test".to_owned(),
            version: "0.1.0".to_owned(),
            api_version: HOST_API_VERSION,
            minimum_host_version: HOST_VERSION.to_owned(),
            runtime,
            entry: runtime.expected_entry().to_owned(),
            pages: vec![panel_plugin_api::PluginPageMetadata {
                id: "test.status".to_owned(),
                title: "Test".to_owned(),
                icon: Some("gauge".to_owned()),
            }],
            permissions: panel_plugin_api::PluginPermissions::default(),
            source: "https://example.com/plugin".to_owned(),
            license: "MIT".to_owned(),
        }
    }

    fn package_entries(runtime: PluginRuntimeKind) -> BTreeMap<String, Vec<u8>> {
        let manifest = manifest(runtime);
        let mut entries = BTreeMap::new();
        entries.insert(
            "manifest.json".to_owned(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        entries.insert("README.md".to_owned(), b"readme".to_vec());
        entries.insert("LICENSE".to_owned(), b"MIT".to_vec());
        entries.insert(
            manifest.entry,
            match runtime {
                PluginRuntimeKind::WasmV1 => b"\0asm\x01\0\0\0".to_vec(),
                PluginRuntimeKind::LuaV1 => b"function metadata() end".to_vec(),
            },
        );
        entries
    }

    fn archive(runtime: PluginRuntimeKind) -> Vec<u8> {
        build_archive(&package_entries(runtime), PackageLimits::default()).unwrap()
    }

    fn verify_dev(bytes: &[u8]) -> Result<VerifiedPackage, PackageError> {
        verify_package_bytes_bound(
            bytes,
            PackageLimits::default(),
            &NoTrustRoots,
            VerificationMode::AllowUnsignedLocal,
            Some(Path::new("/fixture/plugin.info-panel-plugin")),
        )
    }

    fn central_offset(bytes: &[u8]) -> usize {
        read_u32(bytes, bytes.len() - EOCD_SIZE + 16).unwrap() as usize
    }

    fn replace_name(bytes: &mut [u8], old: &[u8], new: &[u8]) {
        assert_eq!(old.len(), new.len());
        let mut found = 0;
        for offset in 0..=bytes.len() - old.len() {
            if &bytes[offset..offset + old.len()] == old {
                bytes[offset..offset + old.len()].copy_from_slice(new);
                found += 1;
            }
        }
        assert_eq!(found, 2);
    }

    #[test]
    fn deterministic_archive_is_byte_reproducible_and_hashes_whole_package() {
        let first = archive(PluginRuntimeKind::WasmV1);
        let second = archive(PluginRuntimeKind::WasmV1);
        assert_eq!(first, second);
        let digest: [u8; 32] = Sha256::digest(&first).into();
        let verified = verify_package_bytes(
            &first,
            PackageLimits::default(),
            &ExactSignature,
            VerificationMode::Signed(&digest),
        )
        .unwrap();
        assert_eq!(verified.sha256(), digest);
        assert_eq!(verified.warning(), None);
    }

    #[test]
    fn signature_provider_fails_closed_without_trust_roots() {
        let bytes = archive(PluginRuntimeKind::WasmV1);
        let error = verify_package_bytes(
            &bytes,
            PackageLimits::default(),
            &NoTrustRoots,
            VerificationMode::Signed(b"signature"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            PackageError::Signature(SignatureError::TrustRootUnavailable)
        ));
    }

    #[test]
    fn explicit_development_mode_returns_persistent_warning() {
        let verified = verify_dev(&archive(PluginRuntimeKind::LuaV1)).unwrap();
        assert_eq!(verified.warning().unwrap().code, DEVELOPMENT_WARNING_CODE);
        let serialized = serde_json::to_string(verified.warning().unwrap()).unwrap();
        assert!(serialized.contains(DEVELOPMENT_WARNING_CODE));
    }

    #[test]
    fn rejects_absolute_parent_backslash_nul_and_non_nfc_paths() {
        for (old, new) in [
            (b"README.md".as_slice(), b"/EADME.md".as_slice()),
            (b"README.md".as_slice(), b"../DME.md".as_slice()),
            (b"README.md".as_slice(), b"READM\\.md".as_slice()),
            (b"README.md".as_slice(), b"READ\0E.md".as_slice()),
        ] {
            let mut bytes = archive(PluginRuntimeKind::WasmV1);
            replace_name(&mut bytes, old, new);
            assert!(matches!(verify_dev(&bytes), Err(PackageError::InvalidPath)));
        }
        assert!(matches!(
            validate_archive_path("assets/e\u{301}.txt".as_bytes(), MAX_PATH_BYTES),
            Err(PackageError::NonCanonicalUnicodePath)
        ));
    }

    #[test]
    fn rejects_symlink_special_or_noncanonical_permissions() {
        let mut bytes = archive(PluginRuntimeKind::WasmV1);
        let central = central_offset(&bytes);
        let symlink_mode = 0o120777_u32 << 16;
        bytes[central + 38..central + 42].copy_from_slice(&symlink_mode.to_le_bytes());
        assert!(matches!(
            verify_dev(&bytes),
            Err(PackageError::NonCanonicalArchive)
        ));
    }

    #[test]
    fn rejects_duplicate_and_case_colliding_paths() {
        let mut case_collision = package_entries(PluginRuntimeKind::WasmV1);
        case_collision.insert("readme.md".to_owned(), b"case".to_vec());
        assert!(matches!(
            build_archive(&case_collision, PackageLimits::default()),
            Err(PackageError::CaseCollidingPath(_))
        ));

        let mut duplicate = package_entries(PluginRuntimeKind::WasmV1);
        duplicate.insert("assets/bar.txt".to_owned(), b"bar".to_vec());
        duplicate.insert("assets/foo.txt".to_owned(), b"foo".to_vec());
        let mut bytes = build_archive(&duplicate, PackageLimits::default()).unwrap();
        replace_name(&mut bytes, b"assets/bar.txt", b"assets/foo.txt");
        assert!(matches!(
            verify_dev(&bytes),
            Err(PackageError::DuplicatePath(_))
        ));
    }

    #[test]
    fn rejects_count_entry_total_and_manifest_size_limits_before_inflation() {
        let bytes = archive(PluginRuntimeKind::WasmV1);
        let limits = PackageLimits {
            max_entries: 3,
            ..PackageLimits::default()
        };
        assert!(matches!(
            verify_package_bytes(
                &bytes,
                limits,
                &NoTrustRoots,
                VerificationMode::AllowUnsignedLocal
            ),
            Err(PackageError::TooManyEntries)
        ));
        let limits = PackageLimits {
            max_entry_bytes: 4,
            max_manifest_bytes: 4,
            ..PackageLimits::default()
        };
        assert!(matches!(
            verify_package_bytes(
                &bytes,
                limits,
                &NoTrustRoots,
                VerificationMode::AllowUnsignedLocal
            ),
            Err(PackageError::EntryTooLarge)
        ));
        let limits = PackageLimits {
            max_total_uncompressed_bytes: 20,
            ..PackageLimits::default()
        };
        assert!(matches!(
            verify_package_bytes(
                &bytes,
                limits,
                &NoTrustRoots,
                VerificationMode::AllowUnsignedLocal
            ),
            Err(PackageError::TotalUncompressedTooLarge)
        ));
        let limits = PackageLimits {
            max_manifest_bytes: 8,
            ..PackageLimits::default()
        };
        assert!(matches!(
            verify_package_bytes(
                &bytes,
                limits,
                &NoTrustRoots,
                VerificationMode::AllowUnsignedLocal
            ),
            Err(PackageError::ManifestTooLarge)
        ));
    }

    #[test]
    fn rejects_ratio_bomb_from_metadata_before_inflation() {
        let mut bytes = archive(PluginRuntimeKind::WasmV1);
        let central = central_offset(&bytes);
        bytes[central + 24..central + 28].copy_from_slice(&1_000_000_u32.to_le_bytes());
        assert!(matches!(
            verify_dev(&bytes),
            Err(PackageError::CompressionRatioTooHigh | PackageError::EntryTooLarge)
        ));
    }

    #[test]
    fn rejects_unsupported_encrypted_and_data_descriptor_entries() {
        let original = archive(PluginRuntimeKind::WasmV1);
        let central = central_offset(&original);
        for flags in [UTF8_FLAG | 1, UTF8_FLAG | (1 << 3)] {
            let mut bytes = original.clone();
            bytes[central + 8..central + 10].copy_from_slice(&flags.to_le_bytes());
            assert!(matches!(
                verify_dev(&bytes),
                Err(PackageError::NonCanonicalArchive)
            ));
        }
        let mut bytes = original;
        bytes[central + 10..central + 12].copy_from_slice(&99_u16.to_le_bytes());
        assert!(matches!(
            verify_dev(&bytes),
            Err(PackageError::UnsupportedCompression(99))
        ));
    }

    #[test]
    fn rejects_extra_executable_payload_and_manifest_entry_mismatch() {
        let mut entries = package_entries(PluginRuntimeKind::WasmV1);
        entries.insert("assets/helper.sh".to_owned(), b"echo unsafe".to_vec());
        let bytes = build_archive(&entries, PackageLimits::default()).unwrap();
        assert!(matches!(
            verify_dev(&bytes),
            Err(PackageError::ExtraExecutablePayload(_))
        ));

        let mut entries = package_entries(PluginRuntimeKind::WasmV1);
        let mut value: serde_json::Value =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        value["runtime"] = serde_json::Value::String("lua@1".to_owned());
        value["entry"] = serde_json::Value::String("plugin.lua".to_owned());
        entries.insert(
            "manifest.json".to_owned(),
            serde_json::to_vec(&value).unwrap(),
        );
        let bytes = build_archive(&entries, PackageLimits::default()).unwrap();
        assert!(verify_dev(&bytes).is_err());
    }

    #[test]
    fn rejects_hidden_bytes_and_local_header_disagreement() {
        let original = archive(PluginRuntimeKind::WasmV1);
        let central = central_offset(&original);
        let mut hidden = original.clone();
        hidden.insert(central, 0);
        let eocd = hidden.len() - EOCD_SIZE;
        hidden[eocd + 16..eocd + 20].copy_from_slice(&((central + 1) as u32).to_le_bytes());
        assert!(matches!(
            verify_dev(&hidden),
            Err(PackageError::HiddenOrOverlappingData | PackageError::MalformedArchive)
        ));

        let mut mismatch = original;
        mismatch[8..10].copy_from_slice(&0_u16.to_le_bytes());
        assert!(matches!(
            verify_dev(&mismatch),
            Err(PackageError::LocalHeaderMismatch(_))
        ));
    }

    #[test]
    fn pack_directory_is_reproducible_across_output_paths() {
        let source = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let manifest = manifest(PluginRuntimeKind::WasmV1);
        fs::write(
            source.path().join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(source.path().join("README.md"), b"readme").unwrap();
        fs::write(source.path().join("LICENSE"), b"MIT").unwrap();
        fs::write(source.path().join("plugin.wasm"), b"\0asm\x01\0\0\0").unwrap();
        let first = output.path().join("first.info-panel-plugin");
        let second = output.path().join("second.info-panel-plugin");
        let first_hash =
            pack_directory(source.path(), &first, None, PackageLimits::default()).unwrap();
        let second_hash =
            pack_directory(source.path(), &second, None, PackageLimits::default()).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
        assert_eq!(first_hash, second_hash);
    }

    #[test]
    fn scaffold_escapes_plugin_name_in_generated_code() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("plugin");
        create_plugin_scaffold(
            &destination,
            "io.github.example.safe",
            "Quoted \\\"name\\\"",
            "https://example.com/plugin",
            PluginRuntimeKind::LuaV1,
        )
        .unwrap();
        let lua = fs::read_to_string(destination.join("plugin.lua")).unwrap();
        assert!(lua.contains("Quoted \\\\\\\"name\\\\\\\""));
    }

    #[test]
    fn rejects_path_prefix_conflicts_in_both_orders() {
        for paths in [
            vec!["assets/a", "assets/a/b"],
            vec!["assets/a/b", "assets/a"],
            vec!["assets/A", "assets/a/b"],
        ] {
            assert!(matches!(
                validate_path_prefixes(paths),
                Err(PackageError::PathPrefixConflict { .. })
            ));
        }
    }

    #[test]
    fn rejects_sigma_final_sigma_and_windows_device_names() {
        for path in [
            "assets/Σ.txt",
            "assets/ς.txt",
            "assets/CON",
            "assets/con.txt",
            "assets/PRN.json",
            "assets/AUX",
            "assets/NUL.png",
            "assets/COM1.txt",
            "assets/com9",
            "assets/LPT1.csv",
            "assets/lpt9.dat",
        ] {
            assert!(matches!(
                validate_archive_path(path.as_bytes(), MAX_PATH_BYTES),
                Err(PackageError::InvalidPath)
            ));
        }
    }

    #[test]
    fn rejects_executable_magic_disguised_as_allowed_assets() {
        let executable_headers: &[&[u8]] = &[
            b"\0asm\x01\0\0\0",
            b"\x7fELF",
            b"MZ",
            &[0xfe, 0xed, 0xfa, 0xce],
            &[0xfe, 0xed, 0xfa, 0xcf],
            &[0xce, 0xfa, 0xed, 0xfe],
            &[0xcf, 0xfa, 0xed, 0xfe],
            &[0xca, 0xfe, 0xba, 0xbe],
            &[0xbe, 0xba, 0xfe, 0xca],
            &[0xca, 0xfe, 0xba, 0xbf],
            &[0xbf, 0xba, 0xfe, 0xca],
        ];
        for header in executable_headers {
            assert!(is_executable_payload("assets/photo.png", header));
        }
    }

    #[test]
    fn schema_validation_is_offline_and_rejects_external_refs() {
        let mut external = package_entries(PluginRuntimeKind::LuaV1);
        external.insert(
            "config.schema.json".to_owned(),
            br#"{"$ref":"https://example.com/schema.json"}"#.to_vec(),
        );
        let bytes = build_archive(&external, PackageLimits::default()).unwrap();
        assert!(matches!(
            verify_dev(&bytes),
            Err(PackageError::ExternalSchemaReference)
        ));

        let mut local = package_entries(PluginRuntimeKind::LuaV1);
        local.insert(
            "config.schema.json".to_owned(),
            br#"{"type":"object","required":["enabled"],"properties":{"enabled":{"type":"boolean"}},"additionalProperties":false}"#.to_vec(),
        );
        let package =
            verify_dev(&build_archive(&local, PackageLimits::default()).unwrap()).unwrap();
        assert!(matches!(
            validate_config_against_schema(&package, b"{}"),
            Err(PackageError::ConfigSchemaRejected)
        ));
        validate_config_against_schema(&package, br#"{"enabled":true}"#).unwrap();
    }

    #[test]
    fn local_runner_warning_binds_path_hash_and_plugin_identity() {
        let package = verify_dev(&archive(PluginRuntimeKind::LuaV1)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lua-runner.wasm");
        fs::write(&path, b"\0asm\x01\0\0\0").unwrap();
        let runner = read_verified_lua_runner(
            &path,
            &package,
            &NoTrustRoots,
            RunnerVerificationMode::AllowUnsignedLocal,
        )
        .unwrap();
        let warning = runner.warning().unwrap();
        assert_eq!(warning.code, LOCAL_RUNNER_WARNING_CODE);
        assert_eq!(warning.plugin_id, package.manifest.id);
        assert_eq!(warning.plugin_version, package.manifest.version);
        assert_eq!(warning.sha256, hex_digest(&runner.sha256()));
        assert_eq!(
            Path::new(&warning.canonical_path),
            fs::canonicalize(path).unwrap()
        );

        assert!(matches!(
            read_verified_lua_runner(
                &directory.path().join("lua-runner.wasm"),
                &package,
                &NoTrustRoots,
                RunnerVerificationMode::TrustedDigest([0_u8; 32]),
            ),
            Err(PackageError::UntrustedLuaRunner)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn staging_has_private_permissions_and_drop_removes_everything() {
        use std::os::unix::fs::PermissionsExt;

        let package = verify_dev(&archive(PluginRuntimeKind::LuaV1)).unwrap();
        let staged_path;
        {
            let staged = package.stage_in_temp().unwrap();
            staged_path = staged.path().to_path_buf();
            assert_eq!(
                fs::metadata(staged.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(staged.path().join("plugin.lua"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(!staged_path.exists());
    }

    #[test]
    fn pack_rejects_output_inside_source_before_reading_manifest() {
        let source = tempfile::tempdir().unwrap();
        let output = source.path().join("nested.info-panel-plugin");
        assert!(matches!(
            pack_directory(source.path(), &output, None, PackageLimits::default()),
            Err(PackageError::OutputInsideSource)
        ));
        assert!(!output.exists());
    }

    #[test]
    fn scaffold_failure_leaves_destination_absent() {
        let parent = tempfile::tempdir().unwrap();
        let destination = parent.path().join("plugin");
        assert!(
            create_plugin_scaffold(
                &destination,
                "invalid id",
                "Plugin",
                "https://example.com/plugin",
                PluginRuntimeKind::LuaV1,
            )
            .is_err()
        );
        assert!(!destination.exists());
    }

    #[test]
    fn rejects_deep_wide_pattern_heavy_and_recursive_schemas() {
        let mut deep = serde_json::Value::Bool(true);
        for _ in 0..=MAX_SCHEMA_DEPTH {
            deep = serde_json::json!({"allOf": [deep]});
        }
        assert!(matches!(
            analyze_schema(&deep),
            Err(PackageError::SchemaTooDeep)
        ));

        let wide_properties = (0..=MAX_SCHEMA_NODES)
            .map(|index| (format!("field{index}"), serde_json::Value::Bool(true)))
            .collect::<serde_json::Map<_, _>>();
        let wide = serde_json::json!({"properties": wide_properties});
        assert!(matches!(
            analyze_schema(&wide),
            Err(PackageError::SchemaTooManyNodes)
        ));

        let patterns = (0..=MAX_SCHEMA_PATTERNS)
            .map(|index| (format!("^field{index}$"), serde_json::Value::Bool(true)))
            .collect::<serde_json::Map<_, _>>();
        let pattern_heavy = serde_json::json!({"patternProperties": patterns});
        assert!(matches!(
            analyze_schema(&pattern_heavy),
            Err(PackageError::SchemaTooManyPatterns)
        ));

        let recursive = serde_json::json!({
            "$defs": {
                "a": {"$ref": "#/$defs/b"},
                "b": {"$ref": "#/$defs/a"}
            },
            "$ref": "#/$defs/a"
        });
        assert!(matches!(
            analyze_schema(&recursive),
            Err(PackageError::RecursiveLocalSchemaReference)
        ));
    }
}
