use std::fs;
use std::path::{Path, PathBuf};

use info_panel_plugin_cli::{
    NoTrustRoots, PackageLimits, RunnerVerificationMode, VerificationMode, pack_directory,
    read_verified_lua_runner, read_verified_package, test_lua_package, test_wasm_package,
};
use panel_plugin_api::PluginPermissions;

fn copy_file(source: impl AsRef<Path>, destination: impl AsRef<Path>) {
    fs::copy(source, destination).expect("fixture file must copy");
}

fn stage_common(source: &Path, destination: &Path) {
    for name in ["manifest.json", "README.md", "LICENSE"] {
        copy_file(source.join(name), destination.join(name));
    }
    let schema = source.join("config.schema.json");
    if schema.exists() {
        copy_file(schema, destination.join("config.schema.json"));
    }
}

#[test]
#[ignore = "requires the example plugin to be built for wasm32-unknown-unknown"]
fn packaged_wasm_plugin_runs_through_restricted_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = root.join("plugins/example-status");
    let wasm = std::env::var_os("INFO_PANEL_EXAMPLE_PLUGIN_WASM")
        .map(PathBuf::from)
        .expect("INFO_PANEL_EXAMPLE_PLUGIN_WASM must point to the built plugin");
    let stage = tempfile::tempdir().expect("stage directory must create");
    stage_common(&source, stage.path());
    let output = tempfile::tempdir().expect("output directory must create");
    let package_path = output.path().join("example.info-panel-plugin");
    pack_directory(
        stage.path(),
        &package_path,
        Some(&wasm),
        PackageLimits::default(),
    )
    .expect("WASM fixture must pack");
    let verified = read_verified_package(
        &package_path,
        PackageLimits::default(),
        &NoTrustRoots,
        VerificationMode::AllowUnsignedLocal,
    )
    .expect("WASM fixture package must validate");
    let grants = PluginPermissions {
        platform: true,
        ..PluginPermissions::default()
    };
    let result = test_wasm_package(&verified, b"{}", &grants).expect("WASM package test must pass");
    assert_eq!(result.plugin_id, "io.github.wangsimiao.example-status");
    assert_eq!(result.page_id, "example.status");
    assert_eq!(result.warnings.len(), 1);
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn packaged_lua_plugin_runs_only_through_restricted_wasm_runner() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = root.join("plugins/example-lua-status");
    let runner = std::env::var_os("INFO_PANEL_LUA_RUNNER_WASM")
        .map(PathBuf::from)
        .expect("INFO_PANEL_LUA_RUNNER_WASM must point to the built runner");
    let stage = tempfile::tempdir().expect("stage directory must create");
    stage_common(&source, stage.path());
    copy_file(source.join("plugin.lua"), stage.path().join("plugin.lua"));
    let output = tempfile::tempdir().expect("output directory must create");
    let package_path = output.path().join("example-lua.info-panel-plugin");
    pack_directory(stage.path(), &package_path, None, PackageLimits::default())
        .expect("Lua fixture must pack");
    let verified = read_verified_package(
        &package_path,
        PackageLimits::default(),
        &NoTrustRoots,
        VerificationMode::AllowUnsignedLocal,
    )
    .expect("Lua fixture package must validate");
    let runner = read_verified_lua_runner(
        &runner,
        &verified,
        &NoTrustRoots,
        RunnerVerificationMode::AllowUnsignedLocal,
    )
    .expect("restricted Lua runner must be verified explicitly");
    let grants = PluginPermissions {
        platform: true,
        ..PluginPermissions::default()
    };
    let result = test_lua_package(&verified, b"{}", &runner, &grants)
        .expect("Lua package test through WASM runner must pass");
    assert_eq!(result.plugin_id, "io.github.wangsimiao.example-lua-status");
    assert_eq!(result.page_id, "example.lua.status");
    assert_eq!(result.warnings.len(), 2);
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn cli_test_command_runs_packaged_lua_with_explicit_grants_and_runner_mode() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = root.join("plugins/example-lua-status");
    let runner = std::env::var_os("INFO_PANEL_LUA_RUNNER_WASM")
        .map(PathBuf::from)
        .expect("INFO_PANEL_LUA_RUNNER_WASM must point to the built runner");
    let stage = tempfile::tempdir().expect("stage directory must create");
    stage_common(&source, stage.path());
    copy_file(source.join("plugin.lua"), stage.path().join("plugin.lua"));
    let output = tempfile::tempdir().expect("output directory must create");
    let package = output.path().join("cli-lua.info-panel-plugin");
    pack_directory(stage.path(), &package, None, PackageLimits::default())
        .expect("Lua fixture must pack");

    let result = std::process::Command::new(env!("CARGO_BIN_EXE_plugin-cli"))
        .arg("test")
        .arg(&package)
        .arg("--allow-unsigned-local")
        .arg("--lua-runner")
        .arg(&runner)
        .arg("--allow-local-runner")
        .arg("--grant-platform")
        .output()
        .expect("plugin-cli must execute");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("tested io.github.wangsimiao.example-lua-status")
    );
}
