use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const LUA_PACKAGE_SHA256: &str = "652c3257a83aae960d32c06890e619eb4e678b206acbe2de3ae1ab15f620c70b";

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_plugin-cli"))
}

fn run(arguments: &[&Path]) -> Output {
    let mut command = Command::new(binary());
    for argument in arguments {
        command.arg(argument);
    }
    command.output().expect("plugin-cli must execute")
}

fn run_strings(arguments: &[&str]) -> Output {
    Command::new(binary())
        .args(arguments)
        .output()
        .expect("plugin-cli must execute")
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pack_lua_fixture(directory: &Path) -> PathBuf {
    let source = project_root().join("plugins/example-lua-status");
    let package = directory.join("example-lua.info-panel-plugin");
    let output = run(&[Path::new("pack"), &source, &package]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(LUA_PACKAGE_SHA256));
    package
}

fn warning_lines(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[test]
fn new_failure_rolls_back_and_success_uses_info_panel_protocol() {
    let parent = tempfile::tempdir().unwrap();
    let failed = parent.path().join("failed");
    let output = run(&[
        Path::new("new"),
        &failed,
        Path::new("--id"),
        Path::new("invalid id"),
        Path::new("--name"),
        Path::new("Invalid"),
        Path::new("--source"),
        Path::new("https://example.com/plugin"),
    ]);
    assert!(!output.status.success());
    assert!(!failed.exists());

    let created = parent.path().join("created");
    let output = run(&[
        Path::new("new"),
        &created,
        Path::new("--id"),
        Path::new("io.github.example.created"),
        Path::new("--name"),
        Path::new("Created"),
        Path::new("--source"),
        Path::new("https://example.com/plugin"),
    ]);
    assert!(output.status.success());
    let manifest = fs::read_to_string(created.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"runtime\": \"lua@1\""));
    assert!(manifest.contains("\"apiVersion\""));
    assert!(
        fs::read_to_string(created.join("README.md"))
            .unwrap()
            .contains("Info Panel")
    );
}

#[test]
fn validate_is_fail_closed_and_emits_bound_json_warning_only_in_dev_mode() {
    let directory = tempfile::tempdir().unwrap();
    let package = pack_lua_fixture(directory.path());

    let output = run(&[Path::new("validate"), &package]);
    assert!(!output.status.success());

    let empty_signature = directory.path().join("empty.sig");
    fs::write(&empty_signature, []).unwrap();
    let output = run(&[
        Path::new("validate"),
        &package,
        Path::new("--signature"),
        &empty_signature,
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("signature is required"));

    let signature = directory.path().join("signature.bin");
    fs::write(&signature, b"signature").unwrap();
    let output = run(&[
        Path::new("validate"),
        &package,
        Path::new("--signature"),
        &signature,
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no plugin signature trust root"));

    let output = run(&[
        Path::new("validate"),
        &package,
        Path::new("--allow-unsigned-local"),
    ]);
    assert!(output.status.success());
    let warnings = warning_lines(&output);
    assert_eq!(warnings.len(), 1);
    let warning = &warnings[0];
    assert_eq!(warning["code"], "plugin.local-unsigned");
    assert_eq!(
        warning["pluginId"],
        "io.github.wangsimiao.example-lua-status"
    );
    assert_eq!(warning["pluginVersion"], "0.1.0");
    assert_eq!(warning["sha256"], LUA_PACKAGE_SHA256);
    assert_eq!(
        warning["canonicalPath"],
        fs::canonicalize(&package)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn test_requires_explicit_permissions_and_runner_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let package = pack_lua_fixture(directory.path());
    let runner = directory.path().join("runner.wasm");
    fs::write(&runner, b"\0asm\x01\0\0\0").unwrap();

    let output = run(&[
        Path::new("test"),
        &package,
        Path::new("--allow-unsigned-local"),
        Path::new("--lua-runner"),
        &runner,
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires exactly one"));

    let output = run(&[
        Path::new("test"),
        &package,
        Path::new("--allow-unsigned-local"),
        Path::new("--lua-runner"),
        &runner,
        Path::new("--runner-sha256"),
        Path::new("0000000000000000000000000000000000000000000000000000000000000000"),
        Path::new("--grant-platform"),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest is not trusted"));

    let output = run(&[
        Path::new("test"),
        &package,
        Path::new("--allow-unsigned-local"),
        Path::new("--lua-runner"),
        &runner,
        Path::new("--allow-local-runner"),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("permissions that were not granted"));
    let warnings = warning_lines(&output);
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[1]["code"], "plugin.local-runner");
    assert_eq!(
        warnings[1]["pluginId"],
        "io.github.wangsimiao.example-lua-status"
    );
    assert_eq!(
        warnings[1]["canonicalPath"],
        fs::canonicalize(runner).unwrap().to_string_lossy().as_ref()
    );
}

#[test]
fn pack_rejects_output_inside_source_and_existing_output() {
    let source = project_root().join("plugins/example-lua-status");
    let inside = source.join("forbidden.info-panel-plugin");
    let output = run(&[Path::new("pack"), &source, &inside]);
    assert!(!output.status.success());
    assert!(!inside.exists());

    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing.info-panel-plugin");
    fs::write(&existing, b"do not replace").unwrap();
    let output = run(&[Path::new("pack"), &source, &existing]);
    assert!(!output.status.success());
    assert_eq!(fs::read(existing).unwrap(), b"do not replace");
}

#[test]
fn help_and_unknown_command_have_stable_exit_codes() {
    assert!(run_strings(&["help"]).status.success());
    assert!(!run_strings(&["unknown-command"]).status.success());
}

#[cfg(unix)]
#[test]
fn auxiliary_inputs_are_nofollow_and_config_is_schema_checked() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let package = pack_lua_fixture(directory.path());
    let runner = directory.path().join("runner.wasm");
    fs::write(&runner, b"\0asm\x01\0\0\0").unwrap();

    let invalid_config = directory.path().join("invalid.json");
    fs::write(&invalid_config, br#"{"unexpected":true}"#).unwrap();
    let output = run(&[
        Path::new("test"),
        &package,
        Path::new("--allow-unsigned-local"),
        Path::new("--config"),
        &invalid_config,
        Path::new("--lua-runner"),
        &runner,
        Path::new("--allow-local-runner"),
        Path::new("--grant-platform"),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.schema.json"));

    let signature_target = directory.path().join("signature-target");
    let signature_link = directory.path().join("signature-link");
    fs::write(&signature_target, b"signature").unwrap();
    symlink(&signature_target, &signature_link).unwrap();
    let output = run(&[
        Path::new("validate"),
        &package,
        Path::new("--signature"),
        &signature_link,
    ]);
    assert!(!output.status.success());

    let config_link = directory.path().join("config-link");
    symlink(&invalid_config, &config_link).unwrap();
    let output = run(&[
        Path::new("test"),
        &package,
        Path::new("--allow-unsigned-local"),
        Path::new("--config"),
        &config_link,
        Path::new("--lua-runner"),
        &runner,
        Path::new("--allow-local-runner"),
        Path::new("--grant-platform"),
    ]);
    assert!(!output.status.success());

    let runner_link = directory.path().join("runner-link");
    symlink(&runner, &runner_link).unwrap();
    let output = run(&[
        Path::new("test"),
        &package,
        Path::new("--allow-unsigned-local"),
        Path::new("--lua-runner"),
        &runner_link,
        Path::new("--allow-local-runner"),
        Path::new("--grant-platform"),
    ]);
    assert!(!output.status.success());
}
