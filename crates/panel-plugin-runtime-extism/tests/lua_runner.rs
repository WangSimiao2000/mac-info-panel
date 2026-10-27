use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use panel_plugin_runtime_extism::{
    LuaWasmRuntime, PluginInstance, PluginRuntime, RuntimeError, RuntimeLimits,
};

const METADATA_REQUEST: &[u8] =
    br#"{"hostApiVersion":{"major":1,"minor":0},"hostVersion":"0.1.0"}"#;

fn runtime() -> LuaWasmRuntime {
    let path = std::env::var_os("INFO_PANEL_LUA_RUNNER_WASM")
        .map(PathBuf::from)
        .expect("INFO_PANEL_LUA_RUNNER_WASM must point to the built runner");
    LuaWasmRuntime::new(std::fs::read(path).expect("failed to read Lua runner"))
        .expect("runner must not be empty")
}

fn call_metadata(source: &[u8], limits: RuntimeLimits) -> Result<Vec<u8>, RuntimeError> {
    let mut plugin = runtime().load_lua(source, limits)?;
    let cancellation = plugin.cancellation_handle();
    plugin.call("metadata", METADATA_REQUEST, &cancellation)
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_instruction_fuel_stops_infinite_loop() {
    let started = Instant::now();
    assert_eq!(
        call_metadata(
            include_bytes!("fixtures/lua/infinite-loop.lua"),
            RuntimeLimits {
                execution_timeout: Duration::from_secs(2),
                max_instructions: 128,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::CallFailed)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_instruction_budget_is_exact_at_one_two_and_completion_boundary() {
    let source = include_bytes!("fixtures/lua/fuel-boundary.lua");
    for max_instructions in [1, 2] {
        assert_eq!(
            call_metadata(
                source,
                RuntimeLimits {
                    max_instructions,
                    ..RuntimeLimits::default()
                },
            ),
            Err(RuntimeError::CallFailed)
        );
    }

    let boundary = (3..256)
        .find(|max_instructions| {
            call_metadata(
                source,
                RuntimeLimits {
                    max_instructions: *max_instructions,
                    ..RuntimeLimits::default()
                },
            )
            .is_ok()
        })
        .expect("minimal fixture must complete within 255 instructions");
    assert_eq!(
        call_metadata(
            source,
            RuntimeLimits {
                max_instructions: boundary - 1,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::CallFailed)
    );
    let output = call_metadata(
        source,
        RuntimeLimits {
            max_instructions: boundary,
            ..RuntimeLimits::default()
        },
    )
    .expect("exact completion boundary must succeed");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output).unwrap()["ok"],
        true
    );
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_wall_time_stops_large_fuel_loop() {
    assert_eq!(
        call_metadata(
            include_bytes!("fixtures/lua/infinite-loop.lua"),
            RuntimeLimits {
                execution_timeout: Duration::from_millis(20),
                max_instructions: u64::MAX,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::Timeout)
    );
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_stack_depth_stops_recursive_overflow() {
    assert_eq!(
        call_metadata(
            include_bytes!("fixtures/lua/recursive-overflow.lua"),
            RuntimeLimits {
                max_instructions: 50_000,
                max_lua_stack_depth: 16,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::CallFailed)
    );
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_stack_is_checked_after_the_final_step() {
    let source = include_bytes!("fixtures/lua/stack-final-overflow.lua");
    assert_eq!(
        call_metadata(
            source,
            RuntimeLimits {
                max_lua_stack_depth: 16,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::CallFailed)
    );
    call_metadata(
        source,
        RuntimeLimits {
            max_lua_stack_depth: 64,
            ..RuntimeLimits::default()
        },
    )
    .expect("same recursion must finish above the tested stack boundary");
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn outer_wasm_memory_stops_single_operation_allocation_bomb() {
    assert_eq!(
        call_metadata(
            include_bytes!("fixtures/lua/allocation-bomb.lua"),
            RuntimeLimits {
                max_memory_bytes: 16 * 1024 * 1024,
                max_lua_memory_bytes: 16 * 1024 * 1024,
                max_instructions: 2_000_000,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::OutOfMemory)
    );
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_output_limit_rejects_oversized_string_and_table() {
    for source in [
        include_bytes!("fixtures/lua/oversized-string.lua").as_slice(),
        include_bytes!("fixtures/lua/oversized-table.lua").as_slice(),
    ] {
        let permissive = call_metadata(
            source,
            RuntimeLimits {
                max_output_bytes: 1024 * 1024,
                ..RuntimeLimits::default()
            },
        )
        .expect("oversized fixture must execute with a permissive output limit");
        assert!(permissive.len() > 1024);
        assert_eq!(
            call_metadata(
                source,
                RuntimeLimits {
                    max_output_bytes: 32,
                    ..RuntimeLimits::default()
                },
            ),
            Err(RuntimeError::CallFailed)
        );
    }
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_forbidden_globals_are_absent() {
    let output = call_metadata(
        include_bytes!("fixtures/lua/forbidden-globals.lua"),
        RuntimeLimits::default(),
    )
    .expect("restricted globals fixture must complete");
    let value: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON output");
    assert_eq!(value["id"], "io.github.example.restricted");
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_arrays_require_bounded_contiguous_positive_indices() {
    for source in [
        include_bytes!("fixtures/lua/sparse-array.lua").as_slice(),
        include_bytes!("fixtures/lua/excessive-array-index.lua").as_slice(),
    ] {
        assert_eq!(
            call_metadata(source, RuntimeLimits::default()),
            Err(RuntimeError::CallFailed)
        );
    }
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_array_marker_is_hidden_and_cannot_be_forged() {
    let output = call_metadata(
        include_bytes!("fixtures/lua/marker-safety.lua"),
        RuntimeLimits::default(),
    )
    .expect("marker fixture must complete");
    let value: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON output");
    assert_eq!(value["marked"], serde_json::json!([]));
    assert_eq!(value["forged"], serde_json::json!({"__json_array": true}));
    assert_eq!(value["markerVisible"], false);
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_random_is_deterministic_per_vm_and_randomseed_without_args_resets_it() {
    let source = include_bytes!("fixtures/lua/random-determinism.lua");
    let first: serde_json::Value = serde_json::from_slice(
        &call_metadata(source, RuntimeLimits::default()).expect("first VM must complete"),
    )
    .unwrap();
    let second: serde_json::Value = serde_json::from_slice(
        &call_metadata(source, RuntimeLimits::default()).expect("second VM must complete"),
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["first"], first["reset"]);
    assert_ne!(first["first"], first["second"]);
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_invalid_return_and_undeclared_network_fail_closed() {
    for source in [
        include_bytes!("fixtures/lua/invalid-return.lua").as_slice(),
        include_bytes!("fixtures/lua/undeclared-network.lua").as_slice(),
    ] {
        assert_eq!(
            call_metadata(source, RuntimeLimits::default()),
            Err(RuntimeError::CallFailed)
        );
    }
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_rejects_nonzero_host_call_budget_until_host_functions_exist() {
    assert_eq!(
        call_metadata(
            include_bytes!("fixtures/lua/forbidden-globals.lua"),
            RuntimeLimits {
                max_host_calls: 1,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::CallFailed)
    );
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_wrapper_invokes_only_documented_entry_points() {
    let mut plugin = runtime()
        .load_lua(
            include_bytes!("fixtures/lua/forbidden-globals.lua"),
            RuntimeLimits::default(),
        )
        .expect("runner must load");
    let cancellation = plugin.cancellation_handle();
    assert_eq!(
        plugin.call("arbitrary_function", b"{}", &cancellation),
        Err(RuntimeError::CallFailed)
    );
}

#[test]
#[ignore = "requires plugins/lua-runner to be built for wasm32-unknown-unknown"]
fn lua_source_and_escaped_wrapper_payload_are_bounded_before_execution() {
    let exact_limit = 1024;
    let exact_source = vec![b'-'; exact_limit];
    runtime()
        .load_lua(
            &exact_source,
            RuntimeLimits {
                max_input_bytes: exact_limit,
                ..RuntimeLimits::default()
            },
        )
        .expect("source exactly at the source limit must load");
    assert!(matches!(
        runtime().load_lua(
            &[b'-'; 1025],
            RuntimeLimits {
                max_input_bytes: exact_limit,
                ..RuntimeLimits::default()
            },
        ),
        Err(RuntimeError::InputTooLarge { limit: 1024 })
    ));

    let escaped_source = vec![b'\\'; 128];
    let mut plugin = runtime()
        .load_lua(
            &escaped_source,
            RuntimeLimits {
                max_input_bytes: 256,
                ..RuntimeLimits::default()
            },
        )
        .expect("raw source fits before JSON escaping");
    let cancellation = plugin.cancellation_handle();
    assert_eq!(
        plugin.call("metadata", b"{}", &cancellation),
        Err(RuntimeError::InputTooLarge { limit: 256 })
    );
}
