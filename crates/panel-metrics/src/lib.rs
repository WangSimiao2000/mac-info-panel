//! Platform-neutral system metric sampling.
//!
//! Platform crates provide cumulative counters through [`MetricsSource`]. [`MetricsSampler`]
//! converts them to snapshots without wrapping subtraction, keeps network baselines per stable
//! interface ID, and derives elapsed time from an injected monotonic clock.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

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

pub trait MetricsSource: Send {
    fn capabilities(&self) -> MetricsCapabilities;
    fn read(&mut self) -> Result<RawMetrics, MetricsError>;
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

/// Stateful converter from cumulative platform counters to normalized metrics.
pub struct MetricsSampler<S, C> {
    source: S,
    clock: C,
    previous_cpu: Option<TimedCpuCounters>,
    previous_network: BTreeMap<String, TimedNetworkCounters>,
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
        }
    }

    pub fn capabilities(&self) -> MetricsCapabilities {
        self.source.capabilities()
    }

    pub fn sample(&mut self) -> Result<SystemMetrics, MetricsError> {
        let sampled_at = self.clock.now();
        let raw = self.source.read()?;

        let cpu_usage_percent = self.sample_cpu(sampled_at, raw.cpu);
        let memory = normalize_memory(raw.memory);
        let disks = normalize_disks(raw.disks);
        let network = self.sample_network(sampled_at, raw.network);

        Ok(SystemMetrics {
            sampled_at,
            cpu_usage_percent,
            memory,
            disks,
            network,
            uptime: raw.uptime,
            load_average: normalize_load(raw.load_average),
            thermal_state: raw.thermal_state,
        })
    }

    /// Drops all delta baselines, for example after sampling was paused while the panel was hidden.
    pub fn reset_deltas(&mut self) {
        self.previous_cpu = None;
        self.previous_network.clear();
    }

    pub fn into_parts(self) -> (S, C) {
        (self.source, self.clock)
    }

    fn sample_cpu(&mut self, sampled_at: Duration, current: Reading<CpuCounters>) -> Reading<f64> {
        let current = match current {
            Reading::Available(value) => value,
            Reading::Unavailable(reason) => {
                self.previous_cpu = None;
                return Reading::Unavailable(reason);
            }
        };

        let previous = self.previous_cpu.replace(TimedCpuCounters {
            at: sampled_at,
            counters: current,
        });
        let Some(previous) = previous else {
            return Reading::Unavailable(UnavailableReason::WarmingUp);
        };
        if sampled_at <= previous.at {
            return Reading::Unavailable(UnavailableReason::NonMonotonicTime);
        }

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
        if total_delta == 0 {
            return Reading::Unavailable(UnavailableReason::InvalidSample);
        }

        let busy_delta = total_delta.saturating_sub(idle_delta);
        Reading::Available(clamp_percent(
            busy_delta as f64 * 100.0 / total_delta as f64,
        ))
    }

    fn sample_network(
        &mut self,
        sampled_at: Duration,
        current: Reading<Vec<NetworkCounters>>,
    ) -> Reading<Vec<NetworkMetrics>> {
        let interfaces = match current {
            Reading::Available(value) => value,
            Reading::Unavailable(reason) => {
                self.previous_network.clear();
                return Reading::Unavailable(reason);
            }
        };

        let ids: BTreeSet<&str> = interfaces
            .iter()
            .map(|interface| interface.id.as_str())
            .collect();
        if ids.len() != interfaces.len() || ids.contains("") {
            self.previous_network.clear();
            return Reading::Unavailable(UnavailableReason::InvalidSample);
        }

        self.previous_network
            .retain(|id, _| ids.contains(id.as_str()));
        let mut metrics = Vec::with_capacity(interfaces.len());

        for interface in interfaces {
            let previous = self.previous_network.insert(
                interface.id.clone(),
                TimedNetworkCounters {
                    at: sampled_at,
                    counters: interface.clone(),
                },
            );
            let rate = previous.map_or(
                Reading::Unavailable(UnavailableReason::WarmingUp),
                |previous| network_rate(sampled_at, &interface, previous),
            );
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
        Reading::Available(raw) if raw.total_bytes > 0 => {
            let used_bytes = raw.total_bytes.saturating_sub(raw.available_bytes);
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
    match raw {
        Reading::Available(disks) => Reading::Available(
            disks
                .into_iter()
                .map(|disk| {
                    let used_bytes = disk.total_bytes.saturating_sub(disk.available_bytes);
                    let usage_percent = if disk.total_bytes == 0 {
                        Reading::Unavailable(UnavailableReason::InvalidSample)
                    } else {
                        Reading::Available(clamp_percent(
                            used_bytes as f64 * 100.0 / disk.total_bytes as f64,
                        ))
                    };
                    DiskMetrics {
                        id: disk.id,
                        name: disk.name,
                        mount_point: disk.mount_point,
                        total_bytes: disk.total_bytes,
                        used_bytes,
                        usage_percent,
                    }
                })
                .collect(),
        ),
        Reading::Unavailable(reason) => Reading::Unavailable(reason),
    }
}

fn normalize_load(raw: Reading<LoadAverage>) -> Reading<LoadAverage> {
    match raw {
        Reading::Available(load)
            if load.one_minute.is_finite()
                && load.five_minutes.is_finite()
                && load.fifteen_minutes.is_finite() =>
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

        fn read(&mut self) -> Result<RawMetrics, MetricsError> {
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
        let Reading::Available(network) = second.network else {
            panic!("network should be available");
        };
        assert_eq!(
            network[0].rate,
            Reading::Unavailable(UnavailableReason::NonMonotonicTime)
        );
    }

    #[test]
    fn clamps_derived_percentages() {
        assert_eq!(
            normalize_memory(Reading::Available(RawMemory {
                total_bytes: 100,
                available_bytes: 200,
            })),
            Reading::Available(MemoryMetrics {
                total_bytes: 100,
                used_bytes: 0,
                usage_percent: 0.0,
            })
        );
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
