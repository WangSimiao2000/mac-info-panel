use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs;
use std::io;
use std::mem::MaybeUninit;
use std::path::Path;
use std::time::Duration;

use panel_metrics::{
    CancellationToken, CpuCounters, LoadAverage, MetricSupport, MetricsCapabilities, MetricsError,
    MetricsSource, NetworkCounters, RawDisk, RawMemory, RawMetrics, Reading, UnavailableReason,
};

#[derive(Debug, Default)]
pub struct LinuxMetricsSource;

impl LinuxMetricsSource {
    pub const fn new() -> Self {
        Self
    }
}

impl MetricsSource for LinuxMetricsSource {
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

    fn read(&mut self, cancellation: &CancellationToken) -> Result<RawMetrics, MetricsError> {
        cancellation.check()?;
        let cpu = read_text("/proc/stat")
            .and_then(|text| parse_cpu(&text))
            .into_reading();
        cancellation.check()?;
        let memory = read_text("/proc/meminfo")
            .and_then(|text| parse_memory(&text))
            .into_reading();
        cancellation.check()?;
        let disks = read_disks(cancellation).into_reading();
        cancellation.check()?;
        let network = read_network(cancellation).into_reading();
        cancellation.check()?;
        let uptime = read_text("/proc/uptime")
            .and_then(|text| parse_uptime(&text))
            .into_reading();
        cancellation.check()?;
        let load_average = read_text("/proc/loadavg")
            .and_then(|text| parse_load(&text))
            .into_reading();
        cancellation.check()?;
        Ok(RawMetrics {
            cpu,
            memory,
            disks,
            network,
            uptime,
            load_average,
            thermal_state: Reading::unsupported(),
        })
    }
}

trait IntoReading<T> {
    fn into_reading(self) -> Reading<T>;
}

impl<T> IntoReading<T> for Result<T, MetricsError> {
    fn into_reading(self) -> Reading<T> {
        match self {
            Ok(value) => Reading::Available(value),
            Err(_) => Reading::Unavailable(UnavailableReason::TemporarilyUnavailable),
        }
    }
}

fn read_text(path: &'static str) -> Result<String, MetricsError> {
    fs::read_to_string(path).map_err(|error| MetricsError::new(path, error.to_string()))
}

fn parse_cpu(text: &str) -> Result<CpuCounters, MetricsError> {
    let line = text
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| MetricsError::new("linux.cpu", "aggregate CPU row is missing"))?;
    let values = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|error| MetricsError::new("linux.cpu", error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() < 4 {
        return Err(MetricsError::new(
            "linux.cpu",
            "aggregate CPU row has too few counters",
        ));
    }
    let total_ticks = values.iter().try_fold(0_u64, |total, value| {
        total
            .checked_add(*value)
            .ok_or_else(|| MetricsError::new("linux.cpu", "CPU counters overflowed"))
    })?;
    let idle_ticks = values[3]
        .checked_add(values.get(4).copied().unwrap_or(0))
        .ok_or_else(|| MetricsError::new("linux.cpu", "idle counters overflowed"))?;
    Ok(CpuCounters {
        total_ticks,
        idle_ticks,
    })
}

fn parse_memory(text: &str) -> Result<RawMemory, MetricsError> {
    let mut total_kib = None;
    let mut available_kib = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("MemTotal:") => total_kib = Some(parse_meminfo_kib(&mut fields, "MemTotal")?),
            Some("MemAvailable:") => {
                available_kib = Some(parse_meminfo_kib(&mut fields, "MemAvailable")?);
            }
            _ => {}
        }
    }
    let total_bytes = kibibytes_to_bytes(total_kib, "MemTotal")?;
    let available_bytes = kibibytes_to_bytes(available_kib, "MemAvailable")?;
    Ok(RawMemory {
        total_bytes,
        available_bytes,
    })
}

fn parse_meminfo_kib<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    field: &'static str,
) -> Result<u64, MetricsError> {
    let value = fields
        .next()
        .ok_or_else(|| MetricsError::new("linux.memory", format!("{field} is missing")))?
        .parse::<u64>()
        .map_err(|error| MetricsError::new("linux.memory", error.to_string()))?;
    if fields.next() != Some("kB") {
        return Err(MetricsError::new(
            "linux.memory",
            format!("{field} does not use kB units"),
        ));
    }
    Ok(value)
}

fn kibibytes_to_bytes(value: Option<u64>, field: &'static str) -> Result<u64, MetricsError> {
    value
        .ok_or_else(|| MetricsError::new("linux.memory", format!("{field} is missing")))?
        .checked_mul(1024)
        .ok_or_else(|| MetricsError::new("linux.memory", format!("{field} overflowed")))
}

#[derive(Debug, PartialEq, Eq)]
struct LinuxMount {
    id: String,
    name: String,
    mount_point: String,
}

fn read_disks(cancellation: &CancellationToken) -> Result<Vec<RawDisk>, MetricsError> {
    let mountinfo = read_text("/proc/self/mountinfo")?;
    let mounts = parse_mountinfo(&mountinfo);
    let mut disks = Vec::with_capacity(mounts.len());
    for mount in mounts {
        cancellation.check()?;
        if let Some((total_bytes, available_bytes)) = filesystem_capacity(&mount.mount_point)? {
            disks.push(RawDisk {
                id: mount.id,
                name: mount.name,
                mount_point: mount.mount_point,
                total_bytes,
                available_bytes,
            });
        }
    }
    cancellation.check()?;
    Ok(disks)
}

fn parse_mountinfo(text: &str) -> Vec<LinuxMount> {
    let mut mounts = Vec::new();
    let mut ids = BTreeSet::new();
    for line in text.lines() {
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        let fields = before.split_whitespace().collect::<Vec<_>>();
        let after_fields = after.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 5 || after_fields.len() < 2 || is_virtual_filesystem(after_fields[0]) {
            continue;
        }
        let root = decode_mount_field(fields[3]);
        let id = format!("linux:{}:{}", fields[2], root);
        // Bind mounts of the same device root intentionally share one identity and display the
        // first mount point reported by the kernel.
        if !ids.insert(id.clone()) {
            continue;
        }
        mounts.push(LinuxMount {
            id,
            name: decode_mount_field(after_fields[1]),
            mount_point: decode_mount_field(fields[4]),
        });
    }
    mounts
}

fn is_virtual_filesystem(kind: &str) -> bool {
    matches!(
        kind,
        "autofs"
            | "binfmt_misc"
            | "bpf"
            | "cgroup"
            | "cgroup2"
            | "configfs"
            | "debugfs"
            | "devpts"
            | "devtmpfs"
            | "efivarfs"
            | "fusectl"
            | "hugetlbfs"
            | "mqueue"
            | "nsfs"
            | "overlay"
            | "proc"
            | "pstore"
            | "ramfs"
            | "rootfs"
            | "rpc_pipefs"
            | "securityfs"
            | "selinuxfs"
            | "squashfs"
            | "sysfs"
            | "tmpfs"
            | "tracefs"
    )
}

fn decode_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn filesystem_capacity(path: &str) -> Result<Option<(u64, u64)>, MetricsError> {
    let path = CString::new(path)
        .map_err(|_| MetricsError::new("linux.disk", "mount point contains a NUL byte"))?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is NUL-terminated and `stats` points to writable storage for `statvfs`.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::EACCES | libc::ENOENT | libc::ENOTCONN) => Ok(None),
            _ => Err(MetricsError::new("linux.disk", error.to_string())),
        };
    }
    // SAFETY: successful `statvfs` initialized the output structure.
    let stats = unsafe { stats.assume_init() };
    let block_size = stats.f_frsize;
    let total_bytes = stats
        .f_blocks
        .checked_mul(block_size)
        .ok_or_else(|| MetricsError::new("linux.disk", "filesystem capacity counter overflowed"))?;
    let available_bytes = stats.f_bavail.checked_mul(block_size).ok_or_else(|| {
        MetricsError::new("linux.disk", "filesystem available counter overflowed")
    })?;
    Ok(Some((total_bytes, available_bytes)))
}

fn read_network(cancellation: &CancellationToken) -> Result<Vec<NetworkCounters>, MetricsError> {
    let text = read_text("/proc/net/dev")?;
    parse_network(&text, cancellation, read_ifindex)
}

fn read_ifindex(name: &str) -> io::Result<u32> {
    let path = Path::new("/sys/class/net").join(name).join("ifindex");
    fs::read_to_string(path)?
        .trim()
        .parse::<u32>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn parse_network(
    text: &str,
    cancellation: &CancellationToken,
    mut ifindex_for: impl FnMut(&str) -> io::Result<u32>,
) -> Result<Vec<NetworkCounters>, MetricsError> {
    let mut interfaces = Vec::new();
    for line in text.lines().skip(2) {
        cancellation.check()?;
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let values = counters.split_whitespace().collect::<Vec<_>>();
        if values.len() < 16 {
            continue;
        }
        let received_bytes = values[0]
            .parse::<u64>()
            .map_err(|error| MetricsError::new("linux.network", error.to_string()))?;
        let transmitted_bytes = values[8]
            .parse::<u64>()
            .map_err(|error| MetricsError::new("linux.network", error.to_string()))?;
        let ifindex = match ifindex_for(name) {
            Ok(value) => value,
            Err(error) if skippable_interface_error(&error) => continue,
            Err(error) => return Err(MetricsError::new("linux.network", error.to_string())),
        };
        interfaces.push(NetworkCounters {
            id: format!("linux:ifindex:{ifindex}"),
            name: name.to_owned(),
            received_bytes,
            transmitted_bytes,
        });
    }
    cancellation.check()?;
    Ok(interfaces)
}

fn skippable_interface_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    ) || matches!(
        error.raw_os_error(),
        Some(libc::ENOENT | libc::ENODEV | libc::EACCES | libc::EPERM)
    )
}

fn parse_uptime(text: &str) -> Result<Duration, MetricsError> {
    let seconds = text
        .split_whitespace()
        .next()
        .ok_or_else(|| MetricsError::new("linux.uptime", "uptime is missing"))?
        .parse::<f64>()
        .map_err(|error| MetricsError::new("linux.uptime", error.to_string()))?;
    if !seconds.is_finite() || seconds < 0.0 || seconds >= u64::MAX as f64 {
        return Err(MetricsError::new("linux.uptime", "uptime is invalid"));
    }
    Duration::try_from_secs_f64(seconds)
        .map_err(|error| MetricsError::new("linux.uptime", error.to_string()))
}

fn parse_load(text: &str) -> Result<LoadAverage, MetricsError> {
    let values = text
        .split_whitespace()
        .take(3)
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|error| MetricsError::new("linux.load", error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != 3
        || values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(MetricsError::new("linux.load", "load average is invalid"));
    }
    Ok(LoadAverage {
        one_minute: values[0],
        five_minutes: values[1],
        fifteen_minutes: values[2],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_cpu_without_double_counting_guest_ticks() {
        let cpu = parse_cpu("cpu  10 2 3 40 5 6 7 8 100 200\n").unwrap();
        assert_eq!(cpu.total_ticks, 81);
        assert_eq!(cpu.idle_ticks, 45);
    }

    #[test]
    fn mountinfo_decodes_escapes_and_deduplicates_bind_mounts() {
        assert_eq!(
            decode_mount_field(r"/a\040b\011c\012d\134e"),
            "/a b\tc\nd\\e"
        );
        let mounts = parse_mountinfo(
            "36 25 8:1 /root\\040dir /mnt\\040one rw - ext4 /dev/sda1 rw\n\
             37 25 8:1 /root\\040dir /mnt/two rw - ext4 /dev/sda1 rw\n\
             38 25 0:42 / /ssh rw - fuse.sshfs host:/ rw\n\
             39 25 0:43 / /overlay rw - overlay overlay rw\n",
        );

        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].id, "linux:8:1:/root dir");
        assert_eq!(mounts[0].mount_point, "/mnt one");
        assert_eq!(mounts[1].id, "linux:0:42:/");
        assert_eq!(mounts[1].mount_point, "/ssh");
    }

    #[test]
    fn meminfo_rejects_wrong_or_missing_units() {
        assert!(parse_memory("MemTotal: 100 MB\nMemAvailable: 50 kB\n").is_err());
        assert!(parse_memory("MemTotal: 100 kB\nMemAvailable: 50\n").is_err());
    }

    #[test]
    fn uptime_rejects_values_outside_duration_range_without_panicking() {
        assert!(parse_uptime("1e300 0\n").is_err());
        assert!(parse_uptime("18446744073709551616 0\n").is_err());
    }

    #[test]
    fn load_rejects_negative_values() {
        assert!(parse_load("-0.1 0.2 0.3 1/1 1\n").is_err());
    }

    #[test]
    fn disappearing_interface_does_not_invalidate_remaining_interfaces() {
        let text = "Inter-| Receive | Transmit\n face |bytes\n\
                    gone: 1 0 0 0 0 0 0 0 2 0 0 0 0 0 0 0\n\
                    keep: 10 0 0 0 0 0 0 0 20 0 0 0 0 0 0 0\n";
        let interfaces = parse_network(text, &CancellationToken::default(), |name| match name {
            "gone" => Err(io::Error::from_raw_os_error(libc::ENODEV)),
            "keep" => Ok(7),
            _ => unreachable!(),
        })
        .unwrap();

        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].id, "linux:ifindex:7");
        assert_eq!(interfaces[0].received_bytes, 10);
        assert_eq!(interfaces[0].transmitted_bytes, 20);
    }

    #[test]
    fn native_source_reports_supported_fields_without_fabricated_values() {
        let mut source = LinuxMetricsSource::new();
        let raw = source.read(&CancellationToken::default()).unwrap();
        assert!(matches!(raw.cpu, Reading::Available(_)));
        assert!(matches!(raw.memory, Reading::Available(_)));
        assert!(matches!(raw.disks, Reading::Available(_)));
        assert!(matches!(raw.network, Reading::Available(_)));
        assert!(matches!(raw.uptime, Reading::Available(_)));
        assert!(matches!(raw.load_average, Reading::Available(_)));
        assert_eq!(raw.thermal_state, Reading::unsupported());
    }
}
