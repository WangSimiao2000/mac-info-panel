use std::cell::Cell;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use panel_http::{AddressClass, NetworkPermission};
use panel_plugin_api::{
    ConfigDiagnostic, HOST_API_VERSION, HOST_VERSION, MetadataRequest, MetadataResponse,
    PackageIdentity, PermissionDelta, PermissionGrant, PluginManifest, PluginRuntimeKind,
    REFRESH_FUNCTION, RefreshRequest, RefreshResponse, VALIDATE_CONFIG_FUNCTION,
    ValidateConfigRequest, ValidateConfigResponse,
};
use panel_plugin_runtime_extism::{
    PluginCancellation, PluginInstance, PluginRuntime, RuntimeError, RuntimeLimits,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use url::Url;

const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_DIAGNOSTICS: usize = 32;
const DEFAULT_MAX_DIAGNOSTIC_CHARS: usize = 512;
const MAX_DIAGNOSTIC_CODE_BYTES: usize = 64;
const MAX_DIAGNOSTIC_PATH_CHARS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostLimits {
    pub runtime: RuntimeLimits,
    pub max_input_bytes: usize,
    pub max_diagnostics: usize,
    pub max_diagnostic_chars: usize,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            runtime: RuntimeLimits::default(),
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_diagnostics: DEFAULT_MAX_DIAGNOSTICS,
            max_diagnostic_chars: DEFAULT_MAX_DIAGNOSTIC_CHARS,
        }
    }
}

impl HostLimits {
    fn validate(self) -> Result<Self, HostError> {
        self.runtime.validate()?;
        if self.max_input_bytes == 0 || self.max_diagnostics == 0 || self.max_diagnostic_chars == 0
        {
            return Err(HostError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackoffPolicy {
    pub initial: Duration,
    pub maximum: Duration,
    pub jitter_ratio_percent: u8,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(5 * 60),
            jitter_ratio_percent: 25,
        }
    }
}

impl BackoffPolicy {
    fn validate(self) -> Result<Self, HostError> {
        if self.initial.is_zero() || self.maximum < self.initial || self.jitter_ratio_percent > 100
        {
            return Err(HostError::InvalidLimits);
        }
        Ok(self)
    }

    fn base_delay(self, failures: u32) -> Duration {
        let exponent = failures.saturating_sub(1).min(31);
        self.initial
            .checked_mul(1_u32 << exponent)
            .unwrap_or(self.maximum)
            .min(self.maximum)
    }
}

pub trait JitterSource: Clone + Send + 'static {
    fn sample(&mut self, upper_bound: Duration) -> Duration;
}

#[derive(Clone, Debug)]
pub struct SystemJitter {
    state: Arc<Mutex<SystemJitterState>>,
}

#[derive(Debug)]
struct SystemJitterState {
    random: u64,
    previous: Option<u64>,
}

impl SystemJitter {
    fn from_seed(seed: u64) -> Self {
        const NON_ZERO_FALLBACK_SEED: u64 = 0xa076_1d64_78bd_642f;
        Self {
            state: Arc::new(Mutex::new(SystemJitterState {
                random: if seed == 0 {
                    NON_ZERO_FALLBACK_SEED
                } else {
                    seed
                },
                previous: None,
            })),
        }
    }
}

impl Default for SystemJitter {
    fn default() -> Self {
        static INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let wall_clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let seed = wall_clock.as_nanos() as u64
            ^ u64::from(std::process::id())
            ^ INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self::from_seed(seed)
    }
}

impl JitterSource for SystemJitter {
    fn sample(&mut self, upper_bound: Duration) -> Duration {
        let upper_nanos = upper_bound.as_nanos().min(u128::from(u64::MAX)) as u64;
        if upper_nanos == 0 {
            return Duration::ZERO;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.random ^= state.random >> 12;
        state.random ^= state.random << 25;
        state.random ^= state.random >> 27;
        let random = state.random.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let modulus = upper_nanos.saturating_add(1);
        let mut sample = random % modulus;
        if state.previous == Some(sample) {
            sample = sample.saturating_add(1) % modulus;
        }
        state.previous = Some(sample);
        Duration::from_nanos(sample)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoJitter;

impl JitterSource for NoJitter {
    fn sample(&mut self, _upper_bound: Duration) -> Duration {
        Duration::ZERO
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshMode {
    Scheduled,
    ManualRetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    Idle,
    Running { generation: u64 },
    BackingOff { retry_at: Duration },
    Cancelled,
}

pub struct PluginLifecycle<J: JitterSource = NoJitter> {
    state: LifecycleState,
    failures: u32,
    manual_retry_available: bool,
    policy: BackoffPolicy,
    jitter: J,
    generation: Arc<AtomicU64>,
    cancelled_generation: Arc<AtomicU64>,
}

impl<J: JitterSource> PluginLifecycle<J> {
    pub fn new(policy: BackoffPolicy, jitter: J) -> Result<Self, HostError> {
        Ok(Self {
            state: LifecycleState::Idle,
            failures: 0,
            manual_retry_available: false,
            policy: policy.validate()?,
            jitter,
            generation: Arc::new(AtomicU64::new(0)),
            cancelled_generation: Arc::new(AtomicU64::new(0)),
        })
    }

    #[must_use]
    pub fn state(&self) -> LifecycleState {
        match self.state {
            LifecycleState::Running { generation }
                if self.cancelled_generation.load(Ordering::SeqCst) >= generation =>
            {
                LifecycleState::Cancelled
            }
            state => state,
        }
    }

    #[must_use]
    pub fn manual_retry_available(&self) -> bool {
        self.manual_retry_available
    }

    pub fn cancel(&mut self) {
        let generation = self.generation.load(Ordering::SeqCst);
        self.cancelled_generation
            .fetch_max(generation, Ordering::SeqCst);
        self.state = LifecycleState::Cancelled;
    }

    pub fn begin(&mut self, now: Duration, mode: RefreshMode) -> Result<u64, HostError> {
        if matches!(self.state, LifecycleState::Running { .. }) {
            return Err(HostError::AlreadyRunning);
        }
        if let LifecycleState::BackingOff { retry_at } = self.state {
            match mode {
                RefreshMode::Scheduled if now < retry_at => {
                    return Err(HostError::BackingOff { retry_at });
                }
                RefreshMode::ManualRetry if !self.manual_retry_available => {
                    return Err(HostError::ManualRetryUnavailable);
                }
                _ => {}
            }
        } else if mode == RefreshMode::ManualRetry {
            return Err(HostError::ManualRetryUnavailable);
        }

        if mode == RefreshMode::ManualRetry {
            self.manual_retry_available = false;
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.state = LifecycleState::Running { generation };
        Ok(generation)
    }

    pub fn succeed(&mut self, generation: u64) -> Result<(), HostError> {
        if let Err(error) = self.ensure_current(generation) {
            self.state = LifecycleState::Cancelled;
            return Err(error);
        }
        self.state = LifecycleState::Idle;
        self.failures = 0;
        self.manual_retry_available = false;
        Ok(())
    }

    pub fn fail(
        &mut self,
        generation: u64,
        now: Duration,
        mode: RefreshMode,
    ) -> Result<Duration, HostError> {
        if let Err(error) = self.ensure_current(generation) {
            self.state = LifecycleState::Cancelled;
            return Err(error);
        }
        self.failures = self.failures.saturating_add(1);
        let base = self.policy.base_delay(self.failures);
        let jitter_bound = base
            .checked_mul(u32::from(self.policy.jitter_ratio_percent))
            .unwrap_or(self.policy.maximum)
            / 100;
        let jitter = self.jitter.sample(jitter_bound).min(jitter_bound);
        let delay = base.saturating_add(jitter).min(self.policy.maximum);
        let retry_at = now.saturating_add(delay);
        self.state = LifecycleState::BackingOff { retry_at };
        self.manual_retry_available = mode == RefreshMode::Scheduled;
        Ok(retry_at)
    }

    fn ensure_current(&self, generation: u64) -> Result<(), HostError> {
        if self.generation.load(Ordering::SeqCst) != generation
            || self.cancelled_generation.load(Ordering::SeqCst) >= generation
        {
            Err(HostError::StaleGeneration)
        } else {
            Ok(())
        }
    }
}

pub struct PluginPackage {
    pub identity: PackageIdentity,
    pub entry: String,
    pub bytes: Vec<u8>,
}

pub struct PluginHost<R: PluginRuntime, J: JitterSource = SystemJitter> {
    runtime: R,
    limits: HostLimits,
    backoff: BackoffPolicy,
    jitter: J,
    host_version: String,
}

impl<R: PluginRuntime> PluginHost<R, SystemJitter> {
    pub fn new(runtime: R, limits: HostLimits) -> Result<Self, HostError> {
        Self::with_jitter(
            runtime,
            limits,
            BackoffPolicy::default(),
            SystemJitter::default(),
            HOST_VERSION,
        )
    }
}

impl<R: PluginRuntime, J: JitterSource> PluginHost<R, J> {
    pub fn with_jitter(
        runtime: R,
        limits: HostLimits,
        backoff: BackoffPolicy,
        jitter: J,
        host_version: impl Into<String>,
    ) -> Result<Self, HostError> {
        Ok(Self {
            runtime,
            limits: limits.validate()?,
            backoff: backoff.validate()?,
            jitter,
            host_version: host_version.into(),
        })
    }

    pub fn load(
        &self,
        manifest: PluginManifest,
        package: PluginPackage,
        grant: &PermissionGrant,
    ) -> Result<LoadedPlugin<R::Instance, J>, HostError> {
        manifest.validate_for_host(&self.host_version, HOST_API_VERSION)?;
        validate_package_identity(&manifest, &package)?;
        validate_grant(&manifest, grant)?;
        if manifest.runtime == PluginRuntimeKind::LuaV1 {
            return Err(HostError::LuaRunnerUnavailable);
        }

        let network_permissions = manifest
            .permissions
            .network
            .iter()
            .map(|origin| network_permission(origin))
            .collect::<Result<Vec<_>, HostError>>()?;
        let mut instance =
            match catch_runtime_unwind(|| self.runtime.load(&package.bytes, self.limits.runtime)) {
                Ok(result) => result?,
                Err(()) => return Err(HostError::RuntimePanicked),
            };
        let instance_valid = Arc::new(AtomicBool::new(true));
        let metadata_cancellation = runtime_cancellation(&instance, &instance_valid)?;
        let metadata: MetadataResponse = call_json(
            &mut instance,
            panel_plugin_api::METADATA_FUNCTION,
            &MetadataRequest {
                host_api_version: HOST_API_VERSION,
                host_version: self.host_version.clone(),
            },
            &metadata_cancellation,
            &instance_valid,
            self.limits.max_input_bytes,
            self.limits.runtime.max_output_bytes,
        )?;
        validate_metadata(&manifest, &metadata)?;

        Ok(LoadedPlugin {
            manifest,
            instance,
            instance_valid,
            limits: self.limits,
            lifecycle: PluginLifecycle::new(self.backoff, self.jitter.clone())?,
            network_permissions,
            validated_config: None,
            diagnostics: VecDeque::new(),
        })
    }
}

pub struct LoadedPlugin<I: PluginInstance, J: JitterSource = SystemJitter> {
    manifest: PluginManifest,
    instance: I,
    instance_valid: Arc<AtomicBool>,
    limits: HostLimits,
    lifecycle: PluginLifecycle<J>,
    network_permissions: Vec<NetworkPermission>,
    validated_config: Option<Value>,
    diagnostics: VecDeque<PluginDiagnostic>,
}

impl<I: PluginInstance, J: JitterSource> LoadedPlugin<I, J> {
    #[must_use]
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    #[must_use]
    pub fn lifecycle_state(&self) -> LifecycleState {
        self.lifecycle.state()
    }

    #[must_use]
    pub fn manual_retry_available(&self) -> bool {
        self.lifecycle.manual_retry_available()
    }

    #[must_use]
    pub fn network_permissions(&self) -> &[NetworkPermission] {
        &self.network_permissions
    }

    #[must_use]
    pub fn diagnostics(&self) -> &VecDeque<PluginDiagnostic> {
        &self.diagnostics
    }

    pub fn validate_config_json(
        &mut self,
        config_json: &[u8],
    ) -> Result<ValidateConfigResponse, HostError> {
        if config_json.len() > self.limits.max_input_bytes {
            return Err(HostError::InputTooLarge);
        }
        let config: Value =
            serde_json::from_slice(config_json).map_err(|_| HostError::InvalidInput)?;
        if !self.manifest.permissions.config && !is_empty_config(&config) {
            return Err(HostError::PermissionDenied);
        }
        let cancellation = runtime_cancellation(&self.instance, &self.instance_valid)?;
        let mut response: ValidateConfigResponse = call_json(
            &mut self.instance,
            VALIDATE_CONFIG_FUNCTION,
            &ValidateConfigRequest {
                config: config.clone(),
            },
            &cancellation,
            &self.instance_valid,
            self.limits.max_input_bytes,
            self.limits.runtime.max_output_bytes,
        )?;
        validate_config_diagnostics(&response, &config, self.limits)?;
        for diagnostic in &mut response.diagnostics {
            diagnostic.message = "Plugin configuration is invalid".into();
        }
        if response.valid {
            self.validated_config = Some(config);
        } else {
            self.validated_config = None;
        }
        Ok(response)
    }

    pub fn begin_refresh(
        &mut self,
        now: Duration,
        mode: RefreshMode,
    ) -> Result<PendingRefresh<I::Cancellation>, HostError> {
        let runtime_cancellation = runtime_cancellation(&self.instance, &self.instance_valid)?;
        let generation = self.lifecycle.begin(now, mode)?;
        Ok(PendingRefresh {
            generation,
            mode,
            cancellation: GenerationCancellation {
                runtime: runtime_cancellation,
                generation,
                cancelled_generation: Arc::clone(&self.lifecycle.cancelled_generation),
                instance_valid: Arc::clone(&self.instance_valid),
            },
        })
    }

    pub fn execute_refresh(
        &mut self,
        pending: PendingRefresh<I::Cancellation>,
        context: RefreshContext,
        now: Duration,
    ) -> Result<RefreshResult, HostError> {
        self.lifecycle.ensure_current(pending.generation)?;
        if self.validated_config.is_none() {
            let _ = self.lifecycle.fail(pending.generation, now, pending.mode);
            return Err(HostError::ConfigNotValidated);
        }
        let request = RefreshRequest {
            generation: pending.generation,
            locale: self.manifest.permissions.locale.then_some(context.locale),
            platform: self
                .manifest
                .permissions
                .platform
                .then_some(context.platform),
            now_unix_ms: context.now_unix_ms,
            config: self
                .manifest
                .permissions
                .config
                .then(|| self.validated_config.clone())
                .flatten(),
        };
        let outcome = call_json::<_, _, RefreshResponse>(
            &mut self.instance,
            REFRESH_FUNCTION,
            &request,
            &pending.cancellation.runtime,
            &self.instance_valid,
            self.limits.max_input_bytes,
            self.limits.runtime.max_output_bytes,
        )
        .and_then(|response| {
            response.page.validate()?;
            if !self
                .manifest
                .pages
                .iter()
                .any(|page| page.id == response.page.id.as_str())
            {
                return Err(HostError::UndeclaredPage);
            }
            Ok(response)
        });

        match outcome {
            Ok(response) => {
                self.lifecycle.succeed(pending.generation)?;
                Ok(RefreshResult {
                    generation: pending.generation,
                    response,
                })
            }
            Err(error) => {
                let retry_at = self.lifecycle.fail(pending.generation, now, pending.mode)?;
                self.record_failure(&error);
                Err(HostError::RefreshFailed { retry_at })
            }
        }
    }

    pub fn refresh(
        &mut self,
        context: RefreshContext,
        now: Duration,
        mode: RefreshMode,
    ) -> Result<RefreshResult, HostError> {
        let pending = self.begin_refresh(now, mode)?;
        self.execute_refresh(pending, context, now)
    }

    fn record_failure(&mut self, error: &HostError) {
        let (code, message) = match error {
            HostError::Runtime(RuntimeError::Timeout) => {
                ("plugin.timeout", "Plugin execution timed out")
            }
            HostError::Runtime(RuntimeError::OutOfMemory) => {
                ("plugin.out-of-memory", "Plugin exceeded its memory limit")
            }
            HostError::Runtime(RuntimeError::OutputTooLarge { .. }) => (
                "plugin.output-too-large",
                "Plugin output exceeded its byte limit",
            ),
            HostError::Runtime(RuntimeError::ErrorTooLarge) => (
                "plugin.error-too-large",
                "Plugin error exceeded its byte limit",
            ),
            HostError::RuntimePanicked => ("plugin.runtime-panic", "Plugin runtime became invalid"),
            HostError::Model(_) | HostError::UndeclaredPage => {
                ("plugin.invalid-page", "Plugin returned an invalid page")
            }
            _ => ("plugin.runtime-error", "Plugin execution failed"),
        };
        if self.diagnostics.len() == self.limits.max_diagnostics {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(PluginDiagnostic {
            code,
            message: message.to_owned(),
        });
    }
}

pub struct PendingRefresh<C: PluginCancellation> {
    generation: u64,
    mode: RefreshMode,
    cancellation: GenerationCancellation<C>,
}

impl<C: PluginCancellation> PendingRefresh<C> {
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn cancellation_handle(&self) -> GenerationCancellation<C> {
        self.cancellation.clone()
    }
}

#[derive(Clone)]
pub struct GenerationCancellation<C: PluginCancellation> {
    runtime: C,
    generation: u64,
    cancelled_generation: Arc<AtomicU64>,
    instance_valid: Arc<AtomicBool>,
}

impl<C: PluginCancellation> GenerationCancellation<C> {
    pub fn cancel(&self) -> Result<(), HostError> {
        self.cancelled_generation
            .fetch_max(self.generation, Ordering::SeqCst);
        if !self.instance_valid.load(Ordering::SeqCst) {
            return Err(HostError::InstanceInvalid);
        }
        match catch_runtime_unwind(|| self.runtime.cancel()) {
            Ok(result) => result.map_err(HostError::from),
            Err(()) => {
                self.instance_valid.store(false, Ordering::SeqCst);
                Err(HostError::RuntimePanicked)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshContext {
    pub locale: String,
    pub platform: String,
    pub now_unix_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RefreshResult {
    pub generation: u64,
    pub response: RefreshResponse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDiagnostic {
    pub code: &'static str,
    pub message: String,
}

fn validate_package_identity(
    manifest: &PluginManifest,
    package: &PluginPackage,
) -> Result<(), HostError> {
    if package.identity.id != manifest.id
        || package.identity.version != manifest.version
        || package.identity.entry != manifest.entry
        || package.entry != manifest.entry
    {
        return Err(HostError::PackageIdentityMismatch);
    }
    Ok(())
}

fn validate_grant(manifest: &PluginManifest, grant: &PermissionGrant) -> Result<(), HostError> {
    grant.permissions.validate()?;
    if grant.plugin_id != manifest.id {
        return Err(HostError::PermissionGrantIdentityMismatch);
    }
    let missing = manifest.permissions.missing_from(&grant.permissions);
    if !missing.is_empty() {
        return Err(HostError::PermissionsNotGranted(missing));
    }
    Ok(())
}

fn validate_metadata(
    manifest: &PluginManifest,
    metadata: &MetadataResponse,
) -> Result<(), HostError> {
    if metadata.id != manifest.id
        || metadata.version != manifest.version
        || metadata.api_version != manifest.api_version
        || metadata.runtime != manifest.runtime
        || metadata.pages != manifest.pages
    {
        return Err(HostError::MetadataMismatch);
    }
    if !metadata.api_version.is_compatible_with(HOST_API_VERSION) {
        return Err(HostError::MetadataMismatch);
    }
    Ok(())
}

fn network_permission(origin: &str) -> Result<NetworkPermission, HostError> {
    let url = Url::parse(origin).map_err(|_| HostError::InvalidNetworkPermission)?;
    let permission =
        NetworkPermission::parse(origin).map_err(|_| HostError::InvalidNetworkPermission)?;
    if url.scheme() == "http" {
        Ok(permission.allow_address_class(AddressClass::Loopback))
    } else {
        Ok(permission)
    }
}

fn runtime_cancellation<I: PluginInstance>(
    instance: &I,
    instance_valid: &Arc<AtomicBool>,
) -> Result<I::Cancellation, HostError> {
    if !instance_valid.load(Ordering::SeqCst) {
        return Err(HostError::InstanceInvalid);
    }
    match catch_runtime_unwind(|| instance.cancellation_handle()) {
        Ok(handle) => Ok(handle),
        Err(()) => {
            instance_valid.store(false, Ordering::SeqCst);
            Err(HostError::RuntimePanicked)
        }
    }
}

fn call_json<I, T, U>(
    instance: &mut I,
    function: &str,
    request: &T,
    cancellation: &I::Cancellation,
    instance_valid: &Arc<AtomicBool>,
    max_input_bytes: usize,
    max_output_bytes: usize,
) -> Result<U, HostError>
where
    I: PluginInstance,
    T: Serialize,
    U: DeserializeOwned,
{
    if !instance_valid.load(Ordering::SeqCst) {
        return Err(HostError::InstanceInvalid);
    }
    let input = encode_json_limited(request, max_input_bytes)?;
    let output = match catch_runtime_unwind(|| instance.call(function, &input, cancellation)) {
        Ok(result) => result?,
        Err(()) => {
            instance_valid.store(false, Ordering::SeqCst);
            return Err(HostError::RuntimePanicked);
        }
    };
    if output.len() > max_output_bytes {
        return Err(HostError::Runtime(RuntimeError::OutputTooLarge {
            limit: max_output_bytes,
        }));
    }
    serde_json::from_slice(&output).map_err(|_| HostError::InvalidOutput)
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(4_096)),
            limit,
            exceeded: false,
        }
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::other("plugin ABI input limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

thread_local! {
    static REDACT_RUNTIME_PANIC: Cell<bool> = const { Cell::new(false) };
}

fn catch_runtime_unwind<F, T>(call: F) -> Result<T, ()>
where
    F: FnOnce() -> T,
{
    static PANIC_HOOK_INSTALLED: OnceLock<()> = OnceLock::new();
    PANIC_HOOK_INSTALLED.get_or_init(|| {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |information| {
            if !REDACT_RUNTIME_PANIC.get() {
                previous_hook(information);
            }
        }));
    });

    struct RedactionGuard(bool);
    impl Drop for RedactionGuard {
        fn drop(&mut self) {
            REDACT_RUNTIME_PANIC.set(self.0);
        }
    }

    let previous = REDACT_RUNTIME_PANIC.replace(true);
    let _guard = RedactionGuard(previous);
    catch_unwind(AssertUnwindSafe(call)).map_err(|_| ())
}

fn encode_json_limited<T: Serialize>(
    value: &T,
    max_input_bytes: usize,
) -> Result<Vec<u8>, HostError> {
    let mut writer = LimitedWriter::new(max_input_bytes);
    if serde_json::to_writer(&mut writer, value).is_err() {
        return if writer.exceeded {
            Err(HostError::InputTooLarge)
        } else {
            Err(HostError::InvalidInput)
        };
    }
    Ok(writer.bytes)
}

fn validate_config_diagnostics(
    response: &ValidateConfigResponse,
    config: &Value,
    limits: HostLimits,
) -> Result<(), HostError> {
    if response.diagnostics.len() > limits.max_diagnostics
        || (response.valid && !response.diagnostics.is_empty())
    {
        return Err(HostError::InvalidOutput);
    }
    for diagnostic in &response.diagnostics {
        validate_config_diagnostic(diagnostic, config, limits.max_diagnostic_chars)?;
    }
    Ok(())
}

fn validate_config_diagnostic(
    diagnostic: &ConfigDiagnostic,
    config: &Value,
    max_message_chars: usize,
) -> Result<(), HostError> {
    let invalid_path = diagnostic.path.as_ref().is_some_and(|path| {
        path.chars().count() > MAX_DIAGNOSTIC_PATH_CHARS
            || !is_canonical_json_pointer(path)
            || config.pointer(path).is_none()
    });
    if diagnostic.code.is_empty()
        || diagnostic.code.len() > MAX_DIAGNOSTIC_CODE_BYTES
        || !diagnostic
            .code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || diagnostic.message.trim().is_empty()
        || diagnostic.message.chars().count() > max_message_chars
        || invalid_path
    {
        return Err(HostError::InvalidOutput);
    }
    Ok(())
}

fn is_canonical_json_pointer(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    if !path.starts_with('/') {
        return false;
    }
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            if index + 1 >= bytes.len() || !matches!(bytes[index + 1], b'0' | b'1') {
                return false;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    true
}

fn is_empty_config(config: &Value) -> bool {
    config.is_null() || config.as_object().is_some_and(serde_json::Map::is_empty)
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Manifest(#[from] panel_plugin_api::ManifestError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Model(#[from] panel_model::ModelError),
    #[error("plugin runtime panicked; instance must be reloaded")]
    RuntimePanicked,
    #[error("plugin instance is invalid and must be reloaded")]
    InstanceInvalid,
    #[error("plugin host limits are invalid")]
    InvalidLimits,
    #[error("plugin package identity does not match its manifest")]
    PackageIdentityMismatch,
    #[error("permission grant belongs to a different plugin")]
    PermissionGrantIdentityMismatch,
    #[error("plugin requests permissions that were not granted")]
    PermissionsNotGranted(PermissionDelta),
    #[error("plugin permission is denied")]
    PermissionDenied,
    #[error("plugin network permission is invalid")]
    InvalidNetworkPermission,
    #[error("lua@1 requires the restricted Lua-in-WASM runner")]
    LuaRunnerUnavailable,
    #[error("plugin metadata does not match its package manifest")]
    MetadataMismatch,
    #[error("plugin configuration has not passed validation")]
    ConfigNotValidated,
    #[error("plugin input exceeds its byte limit")]
    InputTooLarge,
    #[error("plugin input could not be encoded")]
    InvalidInput,
    #[error("plugin returned invalid output")]
    InvalidOutput,
    #[error("plugin returned a page that is not declared in its manifest")]
    UndeclaredPage,
    #[error("plugin refresh is already running")]
    AlreadyRunning,
    #[error("plugin is backing off until {retry_at:?}")]
    BackingOff { retry_at: Duration },
    #[error("manual retry is not available")]
    ManualRetryUnavailable,
    #[error("plugin result belongs to a stale generation")]
    StaleGeneration,
    #[error("plugin refresh failed; next scheduled attempt is at {retry_at:?}")]
    RefreshFailed { retry_at: Duration },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_jitter_replaces_zero_seed_with_non_zero_state() {
        let jitter = SystemJitter::from_seed(0);
        let state = jitter
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_ne!(state.random, 0);
    }
}
