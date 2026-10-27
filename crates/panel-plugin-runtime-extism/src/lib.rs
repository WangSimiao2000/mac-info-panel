use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

use extism::{
    CancelHandle as ExtismCancelHandle, Function, Manifest, PTR, Plugin, UserData, Val, Wasm,
};
use thiserror::Error;

const WASM_PAGE_BYTES: usize = 64 * 1024;
const ERROR_NONE: u8 = 0;
const ERROR_REPORTED: u8 = 1;
const ERROR_TOO_LARGE: u8 = 2;
const ERROR_INVALID: u8 = 3;

pub trait PluginCancellation: Clone + Send + Sync + 'static {
    fn cancel(&self) -> Result<(), RuntimeError>;
}

pub trait PluginInstance: Send {
    type Cancellation: PluginCancellation;

    fn cancellation_handle(&self) -> Self::Cancellation;
    fn call(
        &mut self,
        function: &str,
        input: &[u8],
        cancellation: &Self::Cancellation,
    ) -> Result<Vec<u8>, RuntimeError>;
}

pub trait PluginRuntime {
    type Instance: PluginInstance;

    fn load(&self, wasm: &[u8], limits: RuntimeLimits) -> Result<Self::Instance, RuntimeError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLimits {
    pub execution_timeout: Duration,
    pub max_memory_bytes: usize,
    pub max_output_bytes: usize,
    pub max_error_bytes: usize,
}

impl RuntimeLimits {
    pub fn validate(self) -> Result<Self, RuntimeError> {
        if self.checked_timeout_millis().is_none()
            || self.max_memory_bytes < WASM_PAGE_BYTES
            || self.max_output_bytes == 0
            || self.max_error_bytes == 0
        {
            return Err(RuntimeError::InvalidLimits);
        }
        let pages = self.max_memory_bytes / WASM_PAGE_BYTES;
        if pages > u32::MAX as usize {
            return Err(RuntimeError::InvalidLimits);
        }
        Ok(self)
    }

    fn max_memory_pages(self) -> u32 {
        (self.max_memory_bytes / WASM_PAGE_BYTES) as u32
    }

    fn checked_timeout_millis(self) -> Option<u64> {
        if self.execution_timeout.is_zero() {
            return None;
        }
        let seconds_ms = self.execution_timeout.as_secs().checked_mul(1_000)?;
        let subsec_nanos = u64::from(self.execution_timeout.subsec_nanos());
        let subsec_ms = subsec_nanos.checked_add(999_999)? / 1_000_000;
        seconds_ms.checked_add(subsec_ms)
    }

    fn normalized_timeout(self) -> Duration {
        Duration::from_millis(
            self.checked_timeout_millis()
                .expect("validated runtime timeout has milliseconds"),
        )
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            execution_timeout: Duration::from_secs(2),
            max_memory_bytes: 64 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            max_error_bytes: 8 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExtismRuntime;

const CALL_PENDING: u8 = 0;
const CALL_ACTIVE: u8 = 1;
const CALL_FINISHING: u8 = 2;
const CALL_FINISHED: u8 = 3;
const CALL_CANCELLED: u8 = 4;

pub struct ExtismPlugin {
    plugin: Plugin,
    cancel_handle: ExtismCancelHandle,
    error_kind: Arc<AtomicU8>,
    max_output_bytes: usize,
    execution_timeout: Duration,
}

#[derive(Clone)]
pub struct ExtismCancellation {
    inner: ExtismCancelHandle,
    state: Arc<AtomicU8>,
}

impl PluginCancellation for ExtismCancellation {
    fn cancel(&self) -> Result<(), RuntimeError> {
        loop {
            match self.state.load(Ordering::SeqCst) {
                CALL_PENDING => {
                    if self
                        .state
                        .compare_exchange(
                            CALL_PENDING,
                            CALL_CANCELLED,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                CALL_ACTIVE => {
                    if self
                        .state
                        .compare_exchange(
                            CALL_ACTIVE,
                            CALL_CANCELLED,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        return self
                            .inner
                            .cancel()
                            .map_err(|_| RuntimeError::CancellationFailed);
                    }
                }
                CALL_FINISHING => {
                    if self
                        .state
                        .compare_exchange(
                            CALL_FINISHING,
                            CALL_CANCELLED,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                CALL_FINISHED => return Err(RuntimeError::CancellationTooLate),
                CALL_CANCELLED => return Ok(()),
                _ => return Err(RuntimeError::InvalidCallState),
            }
        }
    }
}

#[derive(Clone)]
struct ErrorBoundary {
    kind: Arc<AtomicU8>,
    max_error_bytes: u64,
}

fn discarded_log_function(name: &'static str) -> Function {
    Function::new(
        name,
        [PTR],
        [],
        UserData::new(()),
        |_plugin, _inputs, _outputs, _data| Ok(()),
    )
    .with_namespace("extism:host/env")
}

impl PluginRuntime for ExtismRuntime {
    type Instance = ExtismPlugin;

    fn load(&self, wasm: &[u8], limits: RuntimeLimits) -> Result<Self::Instance, RuntimeError> {
        if wasm.is_empty() {
            return Err(RuntimeError::EmptyModule);
        }
        let limits = limits.validate()?;
        let manifest = Manifest::new([Wasm::data(wasm)])
            .with_timeout(limits.normalized_timeout())
            .with_memory_max(limits.max_memory_pages())
            .disallow_all_hosts();
        let error_kind = Arc::new(AtomicU8::new(ERROR_NONE));
        let error_boundary = ErrorBoundary {
            kind: Arc::clone(&error_kind),
            max_error_bytes: limits.max_error_bytes as u64,
        };
        let error_set = Function::new(
            "error_set",
            [PTR],
            [],
            UserData::new(error_boundary),
            |plugin, inputs, _outputs, boundary| {
                let offset = inputs
                    .first()
                    .and_then(Val::i64)
                    .ok_or_else(|| extism::Error::msg("invalid plugin error pointer"))?;
                let boundary = boundary
                    .get()?
                    .lock()
                    .map_err(|_| extism::Error::msg("plugin error boundary is unavailable"))?
                    .clone();
                if offset == 0 {
                    return Ok(());
                }
                let Ok(offset) = u64::try_from(offset) else {
                    boundary.kind.fetch_max(ERROR_INVALID, Ordering::SeqCst);
                    return Ok(());
                };
                let kind = match plugin.memory_length(offset) {
                    Ok(length) if length <= boundary.max_error_bytes => ERROR_REPORTED,
                    Ok(_) => ERROR_TOO_LARGE,
                    Err(_) => ERROR_INVALID,
                };
                boundary.kind.fetch_max(kind, Ordering::SeqCst);
                Ok(())
            },
        )
        .with_namespace("extism:host/env");
        let blocked_http = Function::new(
            "http_request",
            [PTR, PTR],
            [PTR],
            UserData::new(()),
            |_plugin, _inputs, outputs, _data| {
                outputs[0] = Val::I64(0);
                Err(extism::Error::msg(
                    "direct plugin HTTP is disabled; use the Info Panel host",
                ))
            },
        )
        .with_namespace("extism:host/env");
        let functions = [
            error_set,
            blocked_http,
            discarded_log_function("log_warn"),
            discarded_log_function("log_info"),
            discarded_log_function("log_debug"),
            discarded_log_function("log_error"),
            discarded_log_function("log_trace"),
        ];
        let plugin =
            Plugin::new(manifest, functions, false).map_err(|_| RuntimeError::LoadFailed)?;
        let cancel_handle = plugin.cancel_handle();
        Ok(ExtismPlugin {
            plugin,
            cancel_handle,
            error_kind,
            max_output_bytes: limits.max_output_bytes,
            execution_timeout: limits.normalized_timeout(),
        })
    }
}

impl PluginInstance for ExtismPlugin {
    type Cancellation = ExtismCancellation;

    fn cancellation_handle(&self) -> Self::Cancellation {
        ExtismCancellation {
            inner: self.cancel_handle.clone(),
            state: Arc::new(AtomicU8::new(CALL_PENDING)),
        }
    }

    fn call(
        &mut self,
        function: &str,
        input: &[u8],
        cancellation: &Self::Cancellation,
    ) -> Result<Vec<u8>, RuntimeError> {
        match cancellation.state.compare_exchange(
            CALL_PENDING,
            CALL_ACTIVE,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => {}
            Err(CALL_CANCELLED) => return Err(RuntimeError::Cancelled),
            Err(_) => return Err(RuntimeError::InvalidCallState),
        }

        self.error_kind.store(ERROR_NONE, Ordering::SeqCst);
        let started = Instant::now();
        let result = self.plugin.call::<&[u8], &[u8]>(function, input);
        if cancellation
            .state
            .compare_exchange(
                CALL_ACTIVE,
                CALL_FINISHING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return Err(RuntimeError::Cancelled);
        }

        let outcome = match self.error_kind.load(Ordering::SeqCst) {
            ERROR_TOO_LARGE => Err(RuntimeError::ErrorTooLarge),
            ERROR_INVALID => Err(RuntimeError::InvalidError),
            ERROR_REPORTED => Err(RuntimeError::CallFailed),
            _ => match result {
                Ok(output) if output.len() > self.max_output_bytes => {
                    Err(RuntimeError::OutputTooLarge {
                        limit: self.max_output_bytes,
                    })
                }
                Ok(output) => Ok(output.to_vec()),
                Err(error) if is_exact_extism_error(&error, "oom") => {
                    Err(RuntimeError::OutOfMemory)
                }
                Err(error) if is_exact_extism_error(&error, "timeout") => {
                    Err(RuntimeError::Timeout)
                }
                Err(_) if started.elapsed() >= self.execution_timeout => Err(RuntimeError::Timeout),
                Err(_) => Err(RuntimeError::CallFailed),
            },
        };

        finish_call(cancellation, outcome)
    }
}

fn finish_call<T>(
    cancellation: &ExtismCancellation,
    outcome: Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    match cancellation.state.compare_exchange(
        CALL_FINISHING,
        CALL_FINISHED,
        Ordering::SeqCst,
        Ordering::SeqCst,
    ) {
        Ok(_) => outcome,
        Err(CALL_CANCELLED) => Err(RuntimeError::Cancelled),
        Err(_) => Err(RuntimeError::InvalidCallState),
    }
}

fn is_exact_extism_error(error: &extism::Error, expected: &str) -> bool {
    error
        .downcast_ref::<String>()
        .is_some_and(|message| message == expected)
        || error
            .downcast_ref::<&'static str>()
            .is_some_and(|message| *message == expected)
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("plugin module is empty")]
    EmptyModule,
    #[error("plugin runtime limits are invalid")]
    InvalidLimits,
    #[error("plugin cancellation state is invalid")]
    InvalidCallState,
    #[error("plugin call already finished before cancellation")]
    CancellationTooLate,
    #[error("plugin could not be loaded")]
    LoadFailed,
    #[error("plugin execution timed out")]
    Timeout,
    #[error("plugin exceeded its memory limit")]
    OutOfMemory,
    #[error("plugin output exceeds the {limit}-byte limit")]
    OutputTooLarge { limit: usize },
    #[error("plugin error exceeds its byte limit")]
    ErrorTooLarge,
    #[error("plugin error pointer is invalid")]
    InvalidError,
    #[error("plugin execution was cancelled")]
    Cancelled,
    #[error("plugin cancellation failed")]
    CancellationFailed,
    #[error("plugin execution failed")]
    CallFailed,
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::*;

    const INFINITE_LOOP: &[u8] = br#"
        (module
            (func (export "run") (result i32)
                (loop $forever (br $forever))
                (i32.const 0)))
    "#;

    const MEMORY_GROWTH: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (func (export "run") (result i32)
                (drop (call $alloc (i64.const 134217728)))
                (i32.const 0)))
    "#;

    const OVERSIZED_OUTPUT: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
            (func (export "run") (result i32)
                (local $output i64)
                (local.set $output (call $alloc (i64.const 64)))
                (call $output_set (local.get $output) (i64.const 64))
                (i32.const 0)))
    "#;

    const PLUGIN_ERROR: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "error_set" (func $error_set (param i64)))
            (func (export "run") (result i32)
                (local $error i64)
                (local.set $error (call $alloc (i64.const 64)))
                (call $error_set (local.get $error))
                (i32.const 0)))
    "#;

    const ERROR_LATCH_OVERSIZED_THEN_ZERO: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "error_set" (func $error_set (param i64)))
            (func (export "run") (result i32)
                (call $error_set (call $alloc (i64.const 64)))
                (call $error_set (i64.const 0))
                (i32.const 0)))
    "#;

    const ERROR_LATCH_OVERSIZED_THEN_SHORT: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "error_set" (func $error_set (param i64)))
            (func (export "run") (result i32)
                (call $error_set (call $alloc (i64.const 64)))
                (call $error_set (call $alloc (i64.const 8)))
                (i32.const 0)))
    "#;

    const ERROR_LATCH_INVALID_THEN_SHORT: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "error_set" (func $error_set (param i64)))
            (func (export "run") (result i32)
                (call $error_set (i64.const -1))
                (call $error_set (call $alloc (i64.const 8)))
                (i32.const 0)))
    "#;

    const ERROR_RESET_BETWEEN_CALLS: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "error_set" (func $error_set (param i64)))
            (func (export "bad") (result i32)
                (call $error_set (call $alloc (i64.const 64)))
                (i32.const 0))
            (func (export "good") (result i32)
                (i32.const 0)))
    "#;

    const UNMEDIATED_HTTP: &[u8] = br#"
        (module
            (import "extism:host/env" "input_offset" (func $input_offset (result i64)))
            (import "extism:host/env" "http_request" (func $http_request (param i64 i64) (result i64)))
            (func (export "run") (result i32)
                (drop (call $http_request (call $input_offset) (i64.const 0)))
                (i32.const 0)))
    "#;

    const UNMEDIATED_LOG: &[u8] = br#"
        (module
            (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
            (import "extism:host/env" "log_error" (func $log_error (param i64)))
            (func (export "run") (result i32)
                (call $log_error (call $alloc (i64.const 64)))
                (i32.const 0)))
    "#;

    fn call_once(
        plugin: &mut ExtismPlugin,
        function: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, RuntimeError> {
        let cancellation = plugin.cancellation_handle();
        plugin.call(function, input, &cancellation)
    }

    #[test]
    fn rejects_empty_modules_and_invalid_limits() {
        assert_eq!(
            ExtismRuntime.load(&[], RuntimeLimits::default()).err(),
            Some(RuntimeError::EmptyModule)
        );
        assert_eq!(
            ExtismRuntime
                .load(
                    b"wasm",
                    RuntimeLimits {
                        max_output_bytes: 0,
                        ..RuntimeLimits::default()
                    }
                )
                .err(),
            Some(RuntimeError::InvalidLimits)
        );
    }

    #[test]
    fn rounds_sub_millisecond_timeouts_up_and_rejects_overflow() {
        let tiny = RuntimeLimits {
            execution_timeout: Duration::from_nanos(1),
            ..RuntimeLimits::default()
        };
        assert_eq!(tiny.checked_timeout_millis(), Some(1));
        let huge = RuntimeLimits {
            execution_timeout: Duration::MAX,
            ..RuntimeLimits::default()
        };
        assert_eq!(huge.validate(), Err(RuntimeError::InvalidLimits));
    }

    #[test]
    fn interrupts_infinite_loops_at_execution_deadline() {
        let mut plugin = ExtismRuntime
            .load(
                INFINITE_LOOP,
                RuntimeLimits {
                    execution_timeout: Duration::from_millis(20),
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut plugin, "run", &[]),
            Err(RuntimeError::Timeout)
        );
    }

    #[test]
    fn enforces_wasm_memory_ceiling() {
        let mut plugin = ExtismRuntime
            .load(
                MEMORY_GROWTH,
                RuntimeLimits {
                    max_memory_bytes: 4 * 1024 * 1024,
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut plugin, "run", &[]),
            Err(RuntimeError::OutOfMemory)
        );
    }

    #[test]
    fn rejects_output_before_copying_it_to_host_memory() {
        let mut plugin = ExtismRuntime
            .load(
                OVERSIZED_OUTPUT,
                RuntimeLimits {
                    max_output_bytes: 32,
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut plugin, "run", &[]),
            Err(RuntimeError::OutputTooLarge { limit: 32 })
        );
    }

    #[test]
    fn intercepts_plugin_errors_before_extism_reads_or_logs_them() {
        let mut oversized = ExtismRuntime
            .load(
                PLUGIN_ERROR,
                RuntimeLimits {
                    max_error_bytes: 32,
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut oversized, "run", &[]),
            Err(RuntimeError::ErrorTooLarge)
        );

        let mut bounded = ExtismRuntime
            .load(
                PLUGIN_ERROR,
                RuntimeLimits {
                    max_error_bytes: 64,
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut bounded, "run", &[]),
            Err(RuntimeError::CallFailed)
        );
    }

    #[test]
    fn error_kind_latches_the_most_severe_state_for_each_call() {
        for wasm in [
            ERROR_LATCH_OVERSIZED_THEN_ZERO,
            ERROR_LATCH_OVERSIZED_THEN_SHORT,
        ] {
            let mut plugin = ExtismRuntime
                .load(
                    wasm,
                    RuntimeLimits {
                        max_error_bytes: 32,
                        ..RuntimeLimits::default()
                    },
                )
                .unwrap();
            assert_eq!(
                call_once(&mut plugin, "run", &[]),
                Err(RuntimeError::ErrorTooLarge)
            );
        }

        let mut invalid = ExtismRuntime
            .load(
                ERROR_LATCH_INVALID_THEN_SHORT,
                RuntimeLimits {
                    max_error_bytes: 32,
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut invalid, "run", &[]),
            Err(RuntimeError::InvalidError)
        );

        let mut reset_between_calls = ExtismRuntime
            .load(
                ERROR_RESET_BETWEEN_CALLS,
                RuntimeLimits {
                    max_error_bytes: 32,
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        assert_eq!(
            call_once(&mut reset_between_calls, "bad", &[]),
            Err(RuntimeError::ErrorTooLarge)
        );
        assert_eq!(
            call_once(&mut reset_between_calls, "good", &[]),
            Ok(Vec::new())
        );
    }

    #[test]
    fn blocks_unmediated_extism_http_without_formatting_its_error() {
        let mut plugin = ExtismRuntime
            .load(UNMEDIATED_HTTP, RuntimeLimits::default())
            .unwrap();
        assert_eq!(
            call_once(
                &mut plugin,
                "run",
                br#"{"url":"https://user:secret@example.com/private"}"#,
            ),
            Err(RuntimeError::CallFailed)
        );
    }

    #[test]
    fn discards_unmediated_extism_logs_without_reading_messages() {
        let mut plugin = ExtismRuntime
            .load(UNMEDIATED_LOG, RuntimeLimits::default())
            .unwrap();
        assert_eq!(call_once(&mut plugin, "run", &[]), Ok(Vec::new()));
    }

    #[test]
    fn cancellation_interrupts_only_its_bound_call() {
        let mut plugin = ExtismRuntime
            .load(
                INFINITE_LOOP,
                RuntimeLimits {
                    execution_timeout: Duration::from_secs(5),
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        let cancellation = plugin.cancellation_handle();
        let worker_cancellation = cancellation.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            started_tx.send(()).unwrap();
            plugin.call("run", &[], &worker_cancellation)
        });
        started_rx.recv().unwrap();
        thread::sleep(Duration::from_millis(30));
        cancellation.cancel().unwrap();
        assert_eq!(worker.join().unwrap(), Err(RuntimeError::Cancelled));
    }

    #[test]
    fn cancellation_before_call_is_not_cleared_across_barrier() {
        let mut plugin = ExtismRuntime
            .load(OVERSIZED_OUTPUT, RuntimeLimits::default())
            .unwrap();
        let cancellation = plugin.cancellation_handle();
        let worker_cancellation = cancellation.clone();
        let ready = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let worker_release = Arc::clone(&release);
        let worker = thread::spawn(move || {
            worker_ready.wait();
            worker_release.wait();
            plugin.call("run", &[], &worker_cancellation)
        });
        ready.wait();
        cancellation.cancel().unwrap();
        release.wait();
        assert_eq!(worker.join().unwrap(), Err(RuntimeError::Cancelled));
    }

    #[test]
    fn cancellation_at_finishing_boundary_cannot_report_false_success() {
        let plugin = ExtismRuntime
            .load(OVERSIZED_OUTPUT, RuntimeLimits::default())
            .unwrap();
        let cancellation = plugin.cancellation_handle();
        cancellation.state.store(CALL_FINISHING, Ordering::SeqCst);
        let worker_cancellation = cancellation.clone();
        let ready = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let worker_release = Arc::clone(&release);
        let worker = thread::spawn(move || {
            worker_ready.wait();
            worker_release.wait();
            finish_call(&worker_cancellation, Ok(Vec::<u8>::new()))
        });
        ready.wait();
        cancellation.cancel().unwrap();
        release.wait();
        assert_eq!(worker.join().unwrap(), Err(RuntimeError::Cancelled));
    }

    #[test]
    fn cancelling_future_token_does_not_interrupt_current_call() {
        let mut plugin = ExtismRuntime
            .load(
                INFINITE_LOOP,
                RuntimeLimits {
                    execution_timeout: Duration::from_secs(5),
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        let current = plugin.cancellation_handle();
        let worker_current = current.clone();
        let future = plugin.cancellation_handle();
        future.cancel().unwrap();
        let ready = Arc::new(std::sync::Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            worker_ready.wait();
            result_tx
                .send(plugin.call("run", &[], &worker_current))
                .unwrap();
        });
        ready.wait();
        thread::sleep(Duration::from_millis(30));
        assert!(matches!(
            result_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        current.cancel().unwrap();
        assert_eq!(result_rx.recv().unwrap(), Err(RuntimeError::Cancelled));
        worker.join().unwrap();
    }

    #[test]
    fn late_old_cancellation_does_not_cancel_new_call_across_barrier() {
        let mut plugin = ExtismRuntime
            .load(
                INFINITE_LOOP,
                RuntimeLimits {
                    execution_timeout: Duration::from_secs(5),
                    ..RuntimeLimits::default()
                },
            )
            .unwrap();
        let old = plugin.cancellation_handle();
        assert_eq!(
            plugin.call("missing", &[], &old),
            Err(RuntimeError::CallFailed)
        );
        let current = plugin.cancellation_handle();
        let worker_current = current.clone();
        let ready = Arc::new(std::sync::Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            worker_ready.wait();
            result_tx
                .send(plugin.call("run", &[], &worker_current))
                .unwrap();
        });
        ready.wait();
        thread::sleep(Duration::from_millis(30));
        assert_eq!(old.cancel(), Err(RuntimeError::CancellationTooLate));
        assert!(matches!(
            result_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        current.cancel().unwrap();
        assert_eq!(result_rx.recv().unwrap(), Err(RuntimeError::Cancelled));
        worker.join().unwrap();
    }
}
