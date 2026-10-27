//! Platform-neutral system metric sampling.
//!
//! Platform crates provide cumulative counters through [`MetricsSource`]. [`MetricsSampler`]
//! converts them to snapshots without wrapping subtraction, keeps network baselines per stable
//! interface ID, and derives elapsed time from an injected monotonic clock.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricSupport {
    Unsupported,
    Available,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsCapabilities {
    pub cpu: MetricSupport,
    pub memory: MetricSupport,
    pub disks: MetricSupport,
    pub network: MetricSupport,
    pub uptime: MetricSupport,
    pub load_average: MetricSupport,
    pub thermal_state: MetricSupport,
}

impl MetricsCapabilities {
    pub const fn unavailable() -> Self {
        Self {
            cpu: MetricSupport::Unsupported,
            memory: MetricSupport::Unsupported,
            disks: MetricSupport::Unsupported,
            network: MetricSupport::Unsupported,
            uptime: MetricSupport::Unsupported,
            load_average: MetricSupport::Unsupported,
            thermal_state: MetricSupport::Unsupported,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    Unsupported,
    TemporarilyUnavailable,
    WarmingUp,
    CounterReset,
    NonMonotonicTime,
    InvalidSample,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reading<T> {
    Available(T),
    Unavailable(UnavailableReason),
}

impl<T> Reading<T> {
    pub const fn unsupported() -> Self {
        Self::Unavailable(UnavailableReason::Unsupported)
    }

    pub fn as_ref(&self) -> Reading<&T> {
        match self {
            Self::Available(value) => Reading::Available(value),
            Self::Unavailable(reason) => Reading::Unavailable(*reason),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuCounters {
    /// Total scheduler ticks, including idle time.
    pub total_ticks: u64,
    /// Idle scheduler ticks included in `total_ticks`.
    pub idle_ticks: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawMemory {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDisk {
    pub id: String,
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkCounters {
    /// Stable platform identifier. Display names are not sufficient identifiers.
    pub id: String,
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadAverage {
    pub one_minute: f64,
    pub five_minutes: f64,
    pub fifteen_minutes: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

/// One raw read from an operating-system metrics source.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMetrics {
    pub cpu: Reading<CpuCounters>,
    pub memory: Reading<RawMemory>,
    pub disks: Reading<Vec<RawDisk>>,
    pub network: Reading<Vec<NetworkCounters>>,
    pub uptime: Reading<Duration>,
    pub load_average: Reading<LoadAverage>,
    pub thermal_state: Reading<ThermalState>,
}

impl RawMetrics {
    pub fn unavailable() -> Self {
        Self {
            cpu: Reading::unsupported(),
            memory: Reading::unsupported(),
            disks: Reading::unsupported(),
            network: Reading::unsupported(),
            uptime: Reading::unsupported(),
            load_average: Reading::unsupported(),
            thermal_state: Reading::unsupported(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiskMetrics {
    pub id: String,
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: Reading<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetworkRate {
    pub received_bytes_per_second: f64,
    pub transmitted_bytes_per_second: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkMetrics {
    pub id: String,
    pub name: String,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
    pub rate: Reading<NetworkRate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SystemMetrics {
    /// Monotonic time since the injected clock's arbitrary epoch.
    pub sampled_at: Duration,
    pub cpu_usage_percent: Reading<f64>,
    pub memory: Reading<MemoryMetrics>,
    pub disks: Reading<Vec<DiskMetrics>>,
    pub network: Reading<Vec<NetworkMetrics>>,
    pub uptime: Reading<Duration>,
    pub load_average: Reading<LoadAverage>,
    pub thermal_state: Reading<ThermalState>,
}

pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
}

/// Production clock with an arbitrary process-local monotonic epoch.
#[derive(Debug)]
pub struct SystemClock {
    epoch: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for SystemClock {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }
}

/// Cloneable scheduler handle that cancels at most the currently active sample.
///
/// Synchronous OS calls are not force-killed. Cancellation instead prevents the sample's atomic
/// completion, so the background scheduler must discard the returned cancellation error and must
/// never publish that late result. The state is one atomic generation counter: even values are
/// idle and odd values identify one active sample. No thread or task is created by cancellation.
#[derive(Clone, Debug, Default)]
pub struct MetricsCancellation {
    state: Arc<AtomicU64>,
}

impl MetricsCancellation {
    fn begin(&self) -> Result<CancellationToken, MetricsError> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if !state.is_multiple_of(2) {
                return Err(MetricsError::new(
                    "metrics.sample",
                    "another sample is already active",
                ));
            }
            let Some(active) = state.checked_add(1).filter(|value| *value < u64::MAX) else {
                return Err(MetricsError::new(
                    "metrics.sample",
                    "cancellation generation exhausted",
                ));
            };
            match self.state.compare_exchange_weak(
                state,
                active,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(CancellationToken {
                        state: Arc::clone(&self.state),
                        generation: active,
                    });
                }
                Err(current) => state = current,
            }
        }
    }

    /// Cancels the active sample. Returns false when no sample is currently active.
    pub fn cancel(&self) -> bool {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state.is_multiple_of(2) {
                return false;
            }
            let Some(cancelled) = state.checked_add(1) else {
                return false;
            };
            match self.state.compare_exchange_weak(
                state,
                cancelled,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(current) => state = current,
            }
        }
    }
}

/// Cancellation token bound to exactly one sample generation.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    state: Arc<AtomicU64>,
    generation: u64,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            state: Arc::new(AtomicU64::new(1)),
            generation: 1,
        }
    }

    pub fn cancel(&self) -> bool {
        let Some(cancelled) = self.generation.checked_add(1) else {
            return false;
        };
        self.state
            .compare_exchange(
                self.generation,
                cancelled,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) != self.generation
    }

    pub fn check(&self) -> Result<(), MetricsError> {
        if self.is_cancelled() {
            Err(MetricsError::cancelled())
        } else {
            Ok(())
        }
    }

    fn complete(&self) -> bool {
        let Some(completed) = self.generation.checked_add(1) else {
            return false;
        };
        self.state
            .compare_exchange(
                self.generation,
                completed,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

pub trait MetricsSource: Send {
    fn capabilities(&self) -> MetricsCapabilities;

    /// Reads one raw snapshot, checking `cancellation` before and between potentially expensive
    /// platform operations. Cancellation must return an error for the whole snapshot.
    fn read(&mut self, cancellation: &CancellationToken) -> Result<RawMetrics, MetricsError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsError {
    pub operation: &'static str,
    pub message: String,
}

impl MetricsError {
    pub fn new(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            operation,
            message: message.into(),
        }
    }

    pub fn cancelled() -> Self {
        Self::new("metrics.cancelled", "metric sample was cancelled")
    }

    pub fn is_cancelled(&self) -> bool {
        self.operation == "metrics.cancelled"
    }
}

impl fmt::Display for MetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for MetricsError {}

#[derive(Clone, Copy, Debug)]
struct TimedCpuCounters {
    at: Duration,
    counters: CpuCounters,
}

#[derive(Clone, Debug)]
struct TimedNetworkCounters {
    at: Duration,
    counters: NetworkCounters,
}

struct PreparedSample {
    metrics: SystemMetrics,
    previous_cpu: Option<TimedCpuCounters>,
    previous_network: BTreeMap<String, TimedNetworkCounters>,
    last_sampled_at: Option<Duration>,
}

/// Stateful converter from cumulative platform counters to normalized metrics.
pub struct MetricsSampler<S, C> {
    source: S,
    clock: C,
    previous_cpu: Option<TimedCpuCounters>,
    previous_network: BTreeMap<String, TimedNetworkCounters>,
    last_sampled_at: Option<Duration>,
    cancellation: MetricsCancellation,
    paused: bool,
}

impl<S, C> MetricsSampler<S, C>
where
    S: MetricsSource,
    C: MonotonicClock,
{
    pub fn new(source: S, clock: C) -> Self {
        Self {
            source,
            clock,
            previous_cpu: None,
            previous_network: BTreeMap::new(),
            last_sampled_at: None,
            cancellation: MetricsCancellation::default(),
            paused: false,
        }
    }

    pub fn capabilities(&self) -> MetricsCapabilities {
        self.source.capabilities()
    }

    /// Applies panel visibility to the sampling policy.
    ///
    /// Returning `None` means sampling should be paused. The first transition from a paused state
    /// back to an active interval clears all counter baselines so hidden time is never reported as
    /// a foreground rate.
    pub fn set_visibility(
        &mut self,
        policy: SamplingPolicy,
        visibility: PanelVisibility,
    ) -> Option<Duration> {
        let interval = policy.interval(visibility);
        let paused = interval.is_none();
        if paused {
            self.cancellation.cancel();
        }
        if self.paused && !paused {
            self.reset_deltas();
        }
        self.paused = paused;
        interval
    }

    /// Returns the stable handle that a background scheduler keeps outside the sampling worker.
    /// Calling it when the panel becomes hidden cancels the active generation without spawning or
    /// retaining a worker thread.
    pub fn cancellation_handle(&self) -> MetricsCancellation {
        self.cancellation.clone()
    }

    pub fn sample(&mut self) -> Result<SystemMetrics, MetricsError> {
        if self.paused {
            return Err(MetricsError::new(
                "metrics.sample",
                "sampling is paused while the panel is hidden",
            ));
        }
        let cancellation = self.cancellation.begin()?;
        let prepared = self.prepare_sample(&cancellation);
        match prepared {
            Ok(prepared) if cancellation.complete() => {
                self.previous_cpu = prepared.previous_cpu;
                self.previous_network = prepared.previous_network;
                self.last_sampled_at = prepared.last_sampled_at;
                Ok(prepared.metrics)
            }
            Ok(_) => Err(MetricsError::cancelled()),
            Err(error) if cancellation.complete() => Err(error),
            Err(_) => Err(MetricsError::cancelled()),
        }
    }

    fn prepare_sample(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<PreparedSample, MetricsError> {
        cancellation.check()?;
        let sampled_at = self.clock.now();
        cancellation.check()?;
        let raw = self.source.read(cancellation)?;
        cancellation.check()?;

        let mut previous_cpu = self.previous_cpu;
        let mut previous_network = self.previous_network.clone();
        let clock_rolled_back = self
            .last_sampled_at
            .is_some_and(|previous| sampled_at <= previous);
        let (cpu_usage_percent, network, last_sampled_at) = if clock_rolled_back {
            previous_cpu = None;
            previous_network.clear();
            (
                Reading::Unavailable(UnavailableReason::NonMonotonicTime),
                Reading::Unavailable(UnavailableReason::NonMonotonicTime),
                None,
            )
        } else {
            (
                Self::sample_cpu(&mut previous_cpu, sampled_at, raw.cpu),
                Self::sample_network(&mut previous_network, sampled_at, raw.network),
                Some(sampled_at),
            )
        };
        let metrics = SystemMetrics {
            sampled_at,
            cpu_usage_percent,
            memory: normalize_memory(raw.memory),
            disks: normalize_disks(raw.disks),
            network,
            uptime: raw.uptime,
            load_average: normalize_load(raw.load_average),
            thermal_state: raw.thermal_state,
        };
        cancellation.check()?;
        Ok(PreparedSample {
            metrics,
            previous_cpu,
            previous_network,
            last_sampled_at,
        })
    }

    /// Drops all delta baselines, for example after sampling was paused while the panel was hidden.
    pub fn reset_deltas(&mut self) {
        self.previous_cpu = None;
        self.previous_network.clear();
        self.last_sampled_at = None;
    }

    pub fn into_parts(self) -> (S, C) {
        (self.source, self.clock)
    }

    fn sample_cpu(
        previous_cpu: &mut Option<TimedCpuCounters>,
        sampled_at: Duration,
        current: Reading<CpuCounters>,
    ) -> Reading<f64> {
        let current = match current {
            Reading::Available(value)
                if value.total_ticks > 0 && value.idle_ticks <= value.total_ticks =>
            {
                value
            }
            Reading::Available(_) => {
                *previous_cpu = None;
                return Reading::Unavailable(UnavailableReason::InvalidSample);
            }
            Reading::Unavailable(reason) => {
                *previous_cpu = None;
                return Reading::Unavailable(reason);
            }
        };

        let Some(previous) = *previous_cpu else {
            *previous_cpu = Some(TimedCpuCounters {
                at: sampled_at,
                counters: current,
            });
            return Reading::Unavailable(UnavailableReason::WarmingUp);
        };
        if sampled_at <= previous.at {
            *previous_cpu = None;
            return Reading::Unavailable(UnavailableReason::NonMonotonicTime);
        }
        *previous_cpu = Some(TimedCpuCounters {
            at: sampled_at,
            counters: current,
        });

        let Some(total_delta) =
            checked_counter_delta(current.total_ticks, previous.counters.total_ticks)
        else {
            return Reading::Unavailable(UnavailableReason::CounterReset);
        };
        let Some(idle_delta) =
            checked_counter_delta(current.idle_ticks, previous.counters.idle_ticks)
        else {
            return Reading::Unavailable(UnavailableReason::CounterReset);
        };
        if total_delta == 0 || idle_delta > total_delta {
            return Reading::Unavailable(UnavailableReason::InvalidSample);
        }

        let busy_delta = total_delta - idle_delta;
        Reading::Available(clamp_percent(
            busy_delta as f64 * 100.0 / total_delta as f64,
        ))
    }

    fn sample_network(
        previous_network: &mut BTreeMap<String, TimedNetworkCounters>,
        sampled_at: Duration,
        current: Reading<Vec<NetworkCounters>>,
    ) -> Reading<Vec<NetworkMetrics>> {
        let interfaces = match current {
            Reading::Available(value) => value,
            Reading::Unavailable(reason) => {
                previous_network.clear();
                return Reading::Unavailable(reason);
            }
        };

        let ids: BTreeSet<&str> = interfaces
            .iter()
            .map(|interface| interface.id.as_str())
            .collect();
        if ids.len() != interfaces.len() || ids.contains("") {
            previous_network.clear();
            return Reading::Unavailable(UnavailableReason::InvalidSample);
        }

        previous_network.retain(|id, _| ids.contains(id.as_str()));
        let mut metrics = Vec::with_capacity(interfaces.len());

        for interface in interfaces {
            let previous = previous_network.get(&interface.id).cloned();
            let rate = match previous {
                Some(previous) if sampled_at <= previous.at => {
                    previous_network.remove(&interface.id);
                    Reading::Unavailable(UnavailableReason::NonMonotonicTime)
                }
                Some(previous) => {
                    previous_network.insert(
                        interface.id.clone(),
                        TimedNetworkCounters {
                            at: sampled_at,
                            counters: interface.clone(),
                        },
                    );
                    network_rate(sampled_at, &interface, previous)
                }
                None => {
                    previous_network.insert(
                        interface.id.clone(),
                        TimedNetworkCounters {
                            at: sampled_at,
                            counters: interface.clone(),
                        },
                    );
                    Reading::Unavailable(UnavailableReason::WarmingUp)
                }
            };
            metrics.push(NetworkMetrics {
                id: interface.id,
                name: interface.name,
                total_received_bytes: interface.received_bytes,
                total_transmitted_bytes: interface.transmitted_bytes,
                rate,
            });
        }

        Reading::Available(metrics)
    }
}

pub const fn checked_counter_delta(current: u64, previous: u64) -> Option<u64> {
    current.checked_sub(previous)
}

fn normalize_memory(raw: Reading<RawMemory>) -> Reading<MemoryMetrics> {
    match raw {
        Reading::Available(raw)
            if raw.total_bytes > 0 && raw.available_bytes <= raw.total_bytes =>
        {
            let used_bytes = raw.total_bytes - raw.available_bytes;
            Reading::Available(MemoryMetrics {
                total_bytes: raw.total_bytes,
                used_bytes,
                usage_percent: clamp_percent(used_bytes as f64 * 100.0 / raw.total_bytes as f64),
            })
        }
        Reading::Available(_) => Reading::Unavailable(UnavailableReason::InvalidSample),
        Reading::Unavailable(reason) => Reading::Unavailable(reason),
    }
}

fn normalize_disks(raw: Reading<Vec<RawDisk>>) -> Reading<Vec<DiskMetrics>> {
    let disks = match raw {
        Reading::Available(disks) => disks,
        Reading::Unavailable(reason) => return Reading::Unavailable(reason),
    };

    let ids: BTreeSet<&str> = disks.iter().map(|disk| disk.id.as_str()).collect();
    if ids.len() != disks.len()
        || ids.contains("")
        || disks
            .iter()
            .any(|disk| disk.total_bytes == 0 || disk.available_bytes > disk.total_bytes)
    {
        return Reading::Unavailable(UnavailableReason::InvalidSample);
    }

    Reading::Available(
        disks
            .into_iter()
            .map(|disk| {
                let used_bytes = disk.total_bytes - disk.available_bytes;
                DiskMetrics {
                    id: disk.id,
                    name: disk.name,
                    mount_point: disk.mount_point,
                    total_bytes: disk.total_bytes,
                    used_bytes,
                    usage_percent: Reading::Available(clamp_percent(
                        used_bytes as f64 * 100.0 / disk.total_bytes as f64,
                    )),
                }
            })
            .collect(),
    )
}

fn normalize_load(raw: Reading<LoadAverage>) -> Reading<LoadAverage> {
    match raw {
        Reading::Available(load)
            if load.one_minute.is_finite()
                && load.one_minute >= 0.0
                && load.five_minutes.is_finite()
                && load.five_minutes >= 0.0
                && load.fifteen_minutes.is_finite()
                && load.fifteen_minutes >= 0.0 =>
        {
            Reading::Available(load)
        }
        Reading::Available(_) => Reading::Unavailable(UnavailableReason::InvalidSample),
        Reading::Unavailable(reason) => Reading::Unavailable(reason),
    }
}

fn network_rate(
    sampled_at: Duration,
    current: &NetworkCounters,
    previous: TimedNetworkCounters,
) -> Reading<NetworkRate> {
    let Some(elapsed) = sampled_at.checked_sub(previous.at) else {
        return Reading::Unavailable(UnavailableReason::NonMonotonicTime);
    };
    let elapsed_seconds = elapsed.as_secs_f64();
    if elapsed_seconds <= 0.0 {
        return Reading::Unavailable(UnavailableReason::NonMonotonicTime);
    }

    let Some(received_delta) =
        checked_counter_delta(current.received_bytes, previous.counters.received_bytes)
    else {
        return Reading::Unavailable(UnavailableReason::CounterReset);
    };
    let Some(transmitted_delta) = checked_counter_delta(
        current.transmitted_bytes,
        previous.counters.transmitted_bytes,
    ) else {
        return Reading::Unavailable(UnavailableReason::CounterReset);
    };

    Reading::Available(NetworkRate {
        received_bytes_per_second: received_delta as f64 / elapsed_seconds,
        transmitted_bytes_per_second: transmitted_delta as f64 / elapsed_seconds,
    })
}

fn clamp_percent(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelVisibility {
    Visible,
    Hidden,
}

/// Sampling cadence selected by panel visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplingPolicy {
    visible_interval: Duration,
    hidden_interval: Option<Duration>,
}

impl SamplingPolicy {
    pub fn new(
        visible_interval: Duration,
        hidden_interval: Option<Duration>,
    ) -> Result<Self, MetricsError> {
        if visible_interval.is_zero() || hidden_interval.is_some_and(|value| value.is_zero()) {
            return Err(MetricsError::new(
                "sampling-policy",
                "sampling intervals must be greater than zero",
            ));
        }
        Ok(Self {
            visible_interval,
            hidden_interval,
        })
    }

    pub fn interval(self, visibility: PanelVisibility) -> Option<Duration> {
        match visibility {
            PanelVisibility::Visible => Some(self.visible_interval),
            PanelVisibility::Hidden => self.hidden_interval,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    struct CancelOnNowClock {
        handle: Arc<Mutex<Option<MetricsCancellation>>>,
        value: Duration,
    }

    impl MonotonicClock for CancelOnNowClock {
        fn now(&self) -> Duration {
            if let Some(handle) = self.handle.lock().unwrap().as_ref() {
                handle.cancel();
            }
            self.value
        }
    }

    struct CountingSource {
        reads: Arc<std::sync::atomic::AtomicUsize>,
        samples: VecDeque<RawMetrics>,
        cancel_during_first_read: bool,
    }

    impl MetricsSource for CountingSource {
        fn capabilities(&self) -> MetricsCapabilities {
            MetricsCapabilities::unavailable()
        }

        fn read(&mut self, cancellation: &CancellationToken) -> Result<RawMetrics, MetricsError> {
            self.reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.cancel_during_first_read {
                self.cancel_during_first_read = false;
                cancellation.cancel();
            }
            self.samples
                .pop_front()
                .ok_or_else(|| MetricsError::new("fake.read", "no sample queued"))
        }
    }

    struct FakeClock {
        values: Mutex<VecDeque<Duration>>,
    }

    impl FakeClock {
        fn new(values: impl IntoIterator<Item = Duration>) -> Self {
            Self {
                values: Mutex::new(values.into_iter().collect()),
            }
        }
    }

    impl MonotonicClock for FakeClock {
        fn now(&self) -> Duration {
            self.values.lock().unwrap().pop_front().unwrap()
        }
    }

    struct FakeSource {
        samples: VecDeque<RawMetrics>,
    }

    impl MetricsSource for FakeSource {
        fn capabilities(&self) -> MetricsCapabilities {
            MetricsCapabilities {
                cpu: MetricSupport::Available,
                memory: MetricSupport::Available,
                disks: MetricSupport::Available,
                network: MetricSupport::Available,
                uptime: MetricSupport::Available,
                load_average: MetricSupport::Available,
                thermal_state: MetricSupport::Unsupported,
            }
        }

        fn read(&mut self, _cancellation: &CancellationToken) -> Result<RawMetrics, MetricsError> {
            self.samples
                .pop_front()
                .ok_or_else(|| MetricsError::new("fake.read", "no sample queued"))
        }
    }

    fn sample(cpu: CpuCounters, network: Vec<NetworkCounters>) -> RawMetrics {
        RawMetrics {
            cpu: Reading::Available(cpu),
            memory: Reading::Available(RawMemory {
                total_bytes: 100,
                available_bytes: 40,
            }),
            disks: Reading::Available(Vec::new()),
            network: Reading::Available(network),
            uptime: Reading::Available(Duration::from_secs(100)),
            load_average: Reading::Available(LoadAverage {
                one_minute: 1.0,
                five_minutes: 0.5,
                fifteen_minutes: 0.25,
            }),
            thermal_state: Reading::unsupported(),
        }
    }

    fn interface(id: &str, received: u64, transmitted: u64) -> NetworkCounters {
        NetworkCounters {
            id: id.into(),
            name: id.into(),
            received_bytes: received,
            transmitted_bytes: transmitted,
        }
    }

    fn sampler(
        samples: impl IntoIterator<Item = RawMetrics>,
        times: impl IntoIterator<Item = Duration>,
    ) -> MetricsSampler<FakeSource, FakeClock> {
        MetricsSampler::new(
            FakeSource {
                samples: samples.into_iter().collect(),
            },
            FakeClock::new(times),
        )
    }

    #[test]
    fn computes_cpu_and_network_deltas_with_fake_clock() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 40,
                    },
                    vec![interface("en0", 1_000, 2_000)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 200,
                        idle_ticks: 65,
                    },
                    vec![interface("en0", 1_500, 2_250)],
                ),
            ],
            [Duration::from_secs(5), Duration::from_secs(10)],
        );

        let first = sampler.sample().unwrap();
        assert_eq!(
            first.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );

        let second = sampler.sample().unwrap();
        assert_eq!(second.cpu_usage_percent, Reading::Available(75.0));
        let Reading::Available(network) = second.network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Available(NetworkRate {
                received_bytes_per_second: 100.0,
                transmitted_bytes_per_second: 50.0,
            })
        );
    }

    #[test]
    fn decreasing_counters_reset_only_their_baselines() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 40,
                    },
                    vec![interface("en0", 100, 100), interface("en1", 200, 200)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 50,
                        idle_ticks: 20,
                    },
                    vec![interface("en0", 50, 50), interface("en1", 300, 400)],
                ),
            ],
            [Duration::from_secs(1), Duration::from_secs(2)],
        );

        sampler.sample().unwrap();
        let second = sampler.sample().unwrap();
        assert_eq!(
            second.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::CounterReset)
        );
        let Reading::Available(network) = second.network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Unavailable(UnavailableReason::CounterReset)
        );
        assert_eq!(
            network[1].rate,
            Reading::Available(NetworkRate {
                received_bytes_per_second: 100.0,
                transmitted_bytes_per_second: 200.0,
            })
        );
    }

    #[test]
    fn interface_appearance_and_disappearance_do_not_reuse_baselines() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 1,
                        idle_ticks: 0,
                    },
                    vec![interface("en0", 100, 100)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 2,
                        idle_ticks: 0,
                    },
                    vec![interface("en1", 500, 500)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 3,
                        idle_ticks: 0,
                    },
                    vec![interface("en0", 1_000, 1_000)],
                ),
            ],
            [
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ],
        );

        sampler.sample().unwrap();
        let second = sampler.sample().unwrap();
        let Reading::Available(second_network) = second.network else {
            panic!("network should be available");
        };
        assert_eq!(
            second_network[0].rate,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );

        let third = sampler.sample().unwrap();
        let Reading::Available(third_network) = third.network else {
            panic!("network should be available");
        };
        assert_eq!(
            third_network[0].rate,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
    }

    #[test]
    fn interface_removal_preserves_other_interface_baselines() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 50,
                    },
                    vec![interface("gone", 100, 100), interface("keep", 200, 300)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 200,
                        idle_ticks: 100,
                    },
                    vec![interface("keep", 300, 500)],
                ),
            ],
            [Duration::from_secs(1), Duration::from_secs(2)],
        );

        sampler.sample().unwrap();
        let second = sampler.sample().unwrap();
        let Reading::Available(network) = second.network else {
            panic!("surviving interface should remain available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Available(NetworkRate {
                received_bytes_per_second: 100.0,
                transmitted_bytes_per_second: 200.0,
            })
        );
    }

    #[test]
    fn non_monotonic_time_never_produces_a_rate() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 50,
                    },
                    vec![interface("en0", 100, 100)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 200,
                        idle_ticks: 100,
                    },
                    vec![interface("en0", 200, 200)],
                ),
            ],
            [Duration::from_secs(2), Duration::from_secs(1)],
        );

        sampler.sample().unwrap();
        let second = sampler.sample().unwrap();
        assert_eq!(
            second.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::NonMonotonicTime)
        );
        assert_eq!(
            second.network,
            Reading::Unavailable(UnavailableReason::NonMonotonicTime)
        );
    }

    #[test]
    fn clock_rollback_clears_baselines_without_trusting_the_rollback_sample() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 50,
                    },
                    vec![interface("en0", 100, 100)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 200,
                        idle_ticks: 100,
                    },
                    vec![interface("en0", 200, 200)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 300,
                        idle_ticks: 150,
                    },
                    vec![interface("en0", 300, 300)],
                ),
            ],
            [
                Duration::from_secs(2),
                Duration::from_secs(1),
                Duration::from_secs(2),
            ],
        );

        sampler.sample().unwrap();
        let rolled_back = sampler.sample().unwrap();
        assert_eq!(
            rolled_back.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::NonMonotonicTime)
        );
        assert_eq!(
            rolled_back.network,
            Reading::Unavailable(UnavailableReason::NonMonotonicTime)
        );

        let recovered = sampler.sample().unwrap();
        assert_eq!(
            recovered.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
        let Reading::Available(network) = recovered.network else {
            panic!("network should restart from a clean baseline");
        };
        assert_eq!(
            network[0].rate,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
    }

    #[test]
    fn rejects_impossible_memory_instead_of_fabricating_zero_usage() {
        assert_eq!(
            normalize_memory(Reading::Available(RawMemory {
                total_bytes: 100,
                available_bytes: 200,
            })),
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
    }

    #[test]
    fn duplicate_interface_ids_invalidate_network_and_clear_baselines() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 1,
                        idle_ticks: 0,
                    },
                    vec![interface("en0", 100, 100)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 2,
                        idle_ticks: 0,
                    },
                    vec![interface("en0", 200, 200), interface("en0", 300, 300)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 3,
                        idle_ticks: 0,
                    },
                    vec![interface("en0", 400, 400)],
                ),
            ],
            [
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ],
        );

        sampler.sample().unwrap();
        assert_eq!(
            sampler.sample().unwrap().network,
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
        let Reading::Available(network) = sampler.sample().unwrap().network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
    }

    #[test]
    fn zero_counter_deltas_produce_zero_rates_but_not_cpu_percentages() {
        let repeated = sample(
            CpuCounters {
                total_ticks: 100,
                idle_ticks: 40,
            },
            vec![interface("en0", 100, 200)],
        );
        let mut sampler = sampler(
            [repeated.clone(), repeated],
            [Duration::from_secs(1), Duration::from_secs(2)],
        );

        sampler.sample().unwrap();
        let second = sampler.sample().unwrap();
        assert_eq!(
            second.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
        let Reading::Available(network) = second.network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Available(NetworkRate {
                received_bytes_per_second: 0.0,
                transmitted_bytes_per_second: 0.0,
            })
        );
    }

    #[test]
    fn zero_capacity_is_invalid_and_percentages_include_both_bounds() {
        assert_eq!(
            normalize_memory(Reading::Available(RawMemory {
                total_bytes: 0,
                available_bytes: 0,
            })),
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
        assert_eq!(
            normalize_disks(Reading::Available(vec![RawDisk {
                id: "empty".into(),
                name: "Empty".into(),
                mount_point: "/empty".into(),
                total_bytes: 0,
                available_bytes: 0,
            }])),
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
        assert_eq!(clamp_percent(-f64::EPSILON), 0.0);
        assert_eq!(clamp_percent(100.1), 100.0);
    }

    #[test]
    fn resuming_after_pause_resets_all_delta_baselines() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 50,
                    },
                    vec![interface("en0", 100, 100)],
                ),
                sample(
                    CpuCounters {
                        total_ticks: 200,
                        idle_ticks: 100,
                    },
                    vec![interface("en0", 300, 300)],
                ),
            ],
            [Duration::from_secs(1), Duration::from_secs(60)],
        );
        let policy = SamplingPolicy::new(Duration::from_secs(1), None).unwrap();

        sampler.sample().unwrap();
        assert_eq!(
            sampler.set_visibility(policy, PanelVisibility::Hidden),
            None
        );
        assert_eq!(
            sampler.set_visibility(policy, PanelVisibility::Visible),
            Some(Duration::from_secs(1))
        );
        let resumed = sampler.sample().unwrap();
        assert_eq!(
            resumed.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
        let Reading::Available(network) = resumed.network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
    }

    #[test]
    fn invalid_disk_identity_or_capacity_invalidates_the_collection() {
        assert_eq!(
            normalize_disks(Reading::Available(vec![RawDisk {
                id: String::new(),
                name: "disk".into(),
                mount_point: "/".into(),
                total_bytes: 100,
                available_bytes: 10,
            }])),
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
        assert_eq!(
            normalize_disks(Reading::Available(vec![RawDisk {
                id: "stable".into(),
                name: "disk".into(),
                mount_point: "/".into(),
                total_bytes: 100,
                available_bytes: 101,
            }])),
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
    }

    #[test]
    fn impossible_idle_delta_is_not_clamped_to_zero_usage() {
        let mut sampler = sampler(
            [
                sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 10,
                    },
                    Vec::new(),
                ),
                sample(
                    CpuCounters {
                        total_ticks: 110,
                        idle_ticks: 30,
                    },
                    Vec::new(),
                ),
            ],
            [Duration::from_secs(1), Duration::from_secs(2)],
        );
        sampler.sample().unwrap();
        assert_eq!(
            sampler.sample().unwrap().cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::InvalidSample)
        );
    }

    #[test]
    fn cancellation_before_source_read_skips_io_and_baselines() {
        let handle_slot = Arc::new(Mutex::new(None));
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut sampler = MetricsSampler::new(
            CountingSource {
                reads: Arc::clone(&reads),
                samples: vec![sample(
                    CpuCounters {
                        total_ticks: 100,
                        idle_ticks: 50,
                    },
                    vec![interface("en0", 100, 100)],
                )]
                .into(),
                cancel_during_first_read: false,
            },
            CancelOnNowClock {
                handle: Arc::clone(&handle_slot),
                value: Duration::from_secs(1),
            },
        );
        *handle_slot.lock().unwrap() = Some(sampler.cancellation_handle());

        assert!(sampler.sample().unwrap_err().is_cancelled());
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn cancellation_during_read_discards_result_and_baseline_updates() {
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let samples = [
            sample(
                CpuCounters {
                    total_ticks: 100,
                    idle_ticks: 50,
                },
                vec![interface("en0", 100, 100)],
            ),
            sample(
                CpuCounters {
                    total_ticks: 200,
                    idle_ticks: 100,
                },
                vec![interface("en0", 200, 200)],
            ),
            sample(
                CpuCounters {
                    total_ticks: 300,
                    idle_ticks: 150,
                },
                vec![interface("en0", 300, 300)],
            ),
        ];
        let mut sampler = MetricsSampler::new(
            CountingSource {
                reads,
                samples: samples.into(),
                cancel_during_first_read: true,
            },
            FakeClock::new([
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
            ]),
        );

        assert!(sampler.sample().unwrap_err().is_cancelled());
        let first_accepted = sampler.sample().unwrap();
        assert_eq!(
            first_accepted.cpu_usage_percent,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
        let Reading::Available(network) = first_accepted.network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Unavailable(UnavailableReason::WarmingUp)
        );
        assert_eq!(
            sampler.sample().unwrap().cpu_usage_percent,
            Reading::Available(50.0)
        );
    }

    #[test]
    fn cancellation_and_completion_have_one_atomic_winner() {
        let controller = MetricsCancellation::default();
        let completed = controller.begin().unwrap();
        assert!(completed.complete());
        assert!(!controller.cancel());

        let cancelled = controller.begin().unwrap();
        assert!(controller.cancel());
        assert!(!cancelled.complete());
        assert!(cancelled.check().unwrap_err().is_cancelled());
    }

    #[test]
    fn hidden_sampling_can_be_reduced_or_paused() {
        let reduced =
            SamplingPolicy::new(Duration::from_secs(1), Some(Duration::from_secs(30))).unwrap();
        assert_eq!(
            reduced.interval(PanelVisibility::Hidden),
            Some(Duration::from_secs(30))
        );

        let paused = SamplingPolicy::new(Duration::from_secs(1), None).unwrap();
        assert_eq!(paused.interval(PanelVisibility::Hidden), None);
        assert_eq!(
            paused.interval(PanelVisibility::Visible),
            Some(Duration::from_secs(1))
        );
    }
}
