use std::ffi::CStr;
use std::mem::{MaybeUninit, size_of};
use std::ptr;
use std::time::Duration;

use mach2::mach_init::mach_host_self;
use mach2::mach_port::mach_port_deallocate;
use mach2::mach_time::{mach_continuous_time, mach_timebase_info, mach_timebase_info_data_t};
use mach2::traps::mach_task_self;
use panel_metrics::{
    CancellationToken, CpuCounters, LoadAverage, MetricSupport, MetricsCapabilities, MetricsError,
    MetricsSource, NetworkCounters, RawDisk, RawMemory, RawMetrics, Reading, UnavailableReason,
};

const KERN_SUCCESS: libc::kern_return_t = 0;
const CPU_STATE_USER: usize = 0;
const CPU_STATE_SYSTEM: usize = 1;
const CPU_STATE_IDLE: usize = 2;
const CPU_STATE_NICE: usize = 3;
const ROUTE_SYSCTL_ATTEMPTS: usize = 3;
const MAX_ROUTE_BYTES: usize = 16 * 1024 * 1024;

unsafe extern "C" {
    #[cfg_attr(target_arch = "x86_64", link_name = "getmntinfo_r_np$INODE64")]
    fn getmntinfo_r_np(mounts: *mut *mut libc::statfs, flags: libc::c_int) -> libc::c_int;
}

struct MachSendRight(libc::mach_port_t);

impl MachSendRight {
    fn host() -> Self {
        // SAFETY: mach_host_self returns a caller-owned send right released by Drop.
        Self(unsafe { mach_host_self() })
    }
}

impl Drop for MachSendRight {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns exactly one send-right reference; the task port is borrowed.
        unsafe {
            mach_port_deallocate(mach_task_self(), self.0);
        }
    }
}

struct MountBuffer(*mut libc::statfs);

impl Drop for MountBuffer {
    fn drop(&mut self) {
        // SAFETY: getmntinfo_r_np transfers a malloc-compatible allocation to the caller.
        unsafe { libc::free(self.0.cast()) };
    }
}

#[derive(Debug, Default)]
pub struct MacOsMetricsSource;

impl MacOsMetricsSource {
    pub const fn new() -> Self {
        Self
    }
}

impl MetricsSource for MacOsMetricsSource {
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
        let cpu = reading(read_cpu_ticks().map(cpu_counters));
        cancellation.check()?;
        let memory = reading(read_memory());
        cancellation.check()?;
        let disks = reading(read_disks(cancellation));
        cancellation.check()?;
        let network = reading(read_network(cancellation));
        cancellation.check()?;
        let uptime = reading(read_uptime());
        cancellation.check()?;
        let load_average = reading(read_load());
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

fn reading<T>(result: Result<T, MetricsError>) -> Reading<T> {
    result.map_or(
        Reading::Unavailable(UnavailableReason::TemporarilyUnavailable),
        Reading::Available,
    )
}

fn read_cpu_ticks() -> Result<[libc::natural_t; libc::CPU_STATE_MAX as usize], MetricsError> {
    let mut info = MaybeUninit::<libc::host_cpu_load_info>::zeroed();
    let mut count = (size_of::<libc::host_cpu_load_info>() / size_of::<libc::integer_t>())
        .try_into()
        .map_err(|_| MetricsError::new("macos.cpu", "host info size is invalid"))?;
    let host = MachSendRight::host();
    // SAFETY: `info` is writable for exactly `count` integer slots and `host` owns a valid Mach
    // send right for the duration of this call.
    let result = unsafe {
        libc::host_statistics(
            host.0,
            libc::HOST_CPU_LOAD_INFO,
            info.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if result != KERN_SUCCESS {
        return Err(MetricsError::new(
            "macos.cpu",
            format!("host_statistics failed with {result}"),
        ));
    }
    // SAFETY: a successful `host_statistics` call initialized the structure.
    Ok(unsafe { info.assume_init().cpu_ticks })
}

fn cpu_counters(ticks: [libc::natural_t; libc::CPU_STATE_MAX as usize]) -> CpuCounters {
    let total_ticks = [
        ticks[CPU_STATE_USER],
        ticks[CPU_STATE_SYSTEM],
        ticks[CPU_STATE_IDLE],
        ticks[CPU_STATE_NICE],
    ]
    .into_iter()
    .map(u64::from)
    .sum();
    CpuCounters {
        total_ticks,
        idle_ticks: u64::from(ticks[CPU_STATE_IDLE]),
    }
}

fn read_memory() -> Result<RawMemory, MetricsError> {
    let mut total_bytes = 0_u64;
    let mut total_size = size_of::<u64>();
    let name = c"hw.memsize";
    // SAFETY: both pointers reference writable values of the sizes supplied to sysctlbyname.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            ptr::from_mut(&mut total_bytes).cast(),
            &mut total_size,
            ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(last_os_error("macos.memory-total"));
    }

    let mut stats = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = (size_of::<libc::vm_statistics64>() / size_of::<libc::integer_t>())
        .try_into()
        .map_err(|_| MetricsError::new("macos.memory", "VM info size is invalid"))?;
    let host = MachSendRight::host();
    // SAFETY: `stats` is writable for exactly `count` integer slots and `host` owns a valid Mach
    // send right for the duration of this call.
    let result = unsafe {
        libc::host_statistics64(
            host.0,
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if result != KERN_SUCCESS {
        return Err(MetricsError::new(
            "macos.memory",
            format!("host_statistics64 failed with {result}"),
        ));
    }
    // SAFETY: a successful `host_statistics64` call initialized the structure.
    let stats = unsafe { stats.assume_init() };
    let available_pages = u64::from(stats.free_count)
        .checked_add(u64::from(stats.inactive_count))
        .and_then(|value| value.checked_add(u64::from(stats.speculative_count)))
        .ok_or_else(|| MetricsError::new("macos.memory", "available pages overflowed"))?;
    // SAFETY: sysconf has no memory-safety preconditions.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = u64::try_from(page_size)
        .map_err(|_| MetricsError::new("macos.memory", "page size is unavailable"))?;
    let available_bytes = available_pages
        .checked_mul(page_size)
        .ok_or_else(|| MetricsError::new("macos.memory", "available bytes overflowed"))?;
    if available_bytes > total_bytes {
        return Err(MetricsError::new(
            "macos.memory",
            "available memory exceeds physical memory",
        ));
    }
    Ok(RawMemory {
        total_bytes,
        available_bytes,
    })
}

fn read_disks(cancellation: &CancellationToken) -> Result<Vec<RawDisk>, MetricsError> {
    cancellation.check()?;
    let mut mounts = ptr::null_mut();
    // SAFETY: getmntinfo_r_np allocates an independent result buffer and writes its pointer here.
    let count = unsafe { getmntinfo_r_np(&mut mounts, libc::MNT_NOWAIT) };
    if count <= 0 || mounts.is_null() {
        return Err(last_os_error("macos.disk"));
    }
    let mounts = MountBuffer(mounts);
    let count = usize::try_from(count)
        .map_err(|_| MetricsError::new("macos.disk", "mount count is invalid"))?;
    // SAFETY: successful getmntinfo_r_np returned `count` initialized records owned by the guard.
    let mount_slice = unsafe { std::slice::from_raw_parts(mounts.0, count) };
    let mut disks = Vec::with_capacity(mount_slice.len());
    for mount in mount_slice {
        cancellation.check()?;
        if is_virtual_filesystem(c_chars(&mount.f_fstypename)) {
            continue;
        }
        let block_size = u64::from(mount.f_bsize);
        let total_bytes = mount
            .f_blocks
            .checked_mul(block_size)
            .ok_or_else(|| MetricsError::new("macos.disk", "capacity counter overflowed"))?;
        let available_bytes = mount
            .f_bavail
            .checked_mul(block_size)
            .ok_or_else(|| MetricsError::new("macos.disk", "available counter overflowed"))?;
        let fsid = fsid_bytes(&mount.f_fsid);
        disks.push(RawDisk {
            id: format!("macos:fsid:{}", hex(&fsid)),
            name: c_chars(&mount.f_mntfromname),
            mount_point: c_chars(&mount.f_mntonname),
            total_bytes,
            available_bytes,
        });
    }
    cancellation.check()?;
    Ok(disks)
}

fn read_route_messages(cancellation: &CancellationToken) -> Result<Vec<u8>, MetricsError> {
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST2, 0];
    let mib_len = libc::c_uint::try_from(mib.len())
        .map_err(|_| MetricsError::new("macos.network", "route MIB length is invalid"))?;
    for _ in 0..ROUTE_SYSCTL_ATTEMPTS {
        cancellation.check()?;
        let mut size = 0_usize;
        // SAFETY: the MIB is valid and this call only writes the required buffer size.
        let query_result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib_len,
                ptr::null_mut(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        if query_result != 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOMEM) {
                continue;
            }
            return Err(last_os_error("macos.network"));
        }
        if size == 0 || size > MAX_ROUTE_BYTES {
            return Err(MetricsError::new(
                "macos.network",
                "route interface buffer size is invalid",
            ));
        }
        cancellation.check()?;
        let mut bytes = vec![0_u8; size];
        // SAFETY: `bytes` has the capacity reported by the immediately preceding sysctl query.
        let read_result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib_len,
                bytes.as_mut_ptr().cast(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        if read_result == 0 {
            if size > bytes.len() {
                return Err(MetricsError::new(
                    "macos.network",
                    "route interface read exceeded its allocation",
                ));
            }
            bytes.truncate(size);
            cancellation.check()?;
            return Ok(bytes);
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return Err(last_os_error("macos.network"));
        }
    }
    Err(MetricsError::new(
        "macos.network",
        "route interface table changed during every bounded read attempt",
    ))
}
fn is_virtual_filesystem(kind: String) -> bool {
    matches!(
        kind.as_str(),
        "autofs" | "devfs" | "fdesc" | "procfs" | "synthetic"
    )
}

fn fsid_bytes(fsid: &libc::fsid_t) -> [u8; size_of::<libc::fsid_t>()] {
    let mut bytes = [0; size_of::<libc::fsid_t>()];
    // SAFETY: source and destination have the same size and do not overlap.
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::from_ref(fsid).cast::<u8>(),
            bytes.as_mut_ptr(),
            bytes.len(),
        );
    }
    bytes
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn c_chars<const N: usize>(value: &[libc::c_char; N]) -> String {
    let bytes = value
        .iter()
        .map(|value| value.cast_unsigned())
        .take_while(|value| *value != 0)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn read_network(cancellation: &CancellationToken) -> Result<Vec<NetworkCounters>, MetricsError> {
    let bytes = read_route_messages(cancellation)?;
    parse_route_messages(&bytes, cancellation)
}

fn parse_route_messages(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<Vec<NetworkCounters>, MetricsError> {
    let mut interfaces = Vec::new();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        cancellation.check()?;
        if bytes.len() - offset < 4 {
            return Err(MetricsError::new(
                "macos.network",
                "route interface message has a truncated header",
            ));
        }
        let length = usize::from(u16::from_ne_bytes([bytes[offset], bytes[offset + 1]]));
        let end = offset.checked_add(length).ok_or_else(|| {
            MetricsError::new("macos.network", "route interface message length overflowed")
        })?;
        if length < 4 || end > bytes.len() {
            return Err(MetricsError::new(
                "macos.network",
                "route interface message has an invalid length",
            ));
        }
        if i32::from(bytes[offset + 3]) == libc::RTM_IFINFO2
            && length >= size_of::<libc::if_msghdr2>()
        {
            // SAFETY: message bounds were checked and route messages may be unaligned.
            let header = unsafe {
                ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<libc::if_msghdr2>())
            };
            let index = u32::from(header.ifm_index);
            let name = interface_name(index)?;
            let data = header.ifm_data;
            interfaces.push(NetworkCounters {
                id: format!("macos:ifindex:{index}"),
                name,
                received_bytes: data.ifi_ibytes,
                transmitted_bytes: data.ifi_obytes,
            });
        }
        offset = end;
    }
    if offset != bytes.len() {
        return Err(MetricsError::new(
            "macos.network",
            "route interface message stream did not end at the buffer boundary",
        ));
    }
    cancellation.check()?;
    Ok(interfaces)
}

fn interface_name(index: u32) -> Result<String, MetricsError> {
    let mut name = [0_i8; libc::IFNAMSIZ];
    // SAFETY: `name` is writable for IFNAMSIZ bytes and index came from the kernel route table.
    let result = unsafe { libc::if_indextoname(index, name.as_mut_ptr()) };
    if result.is_null() {
        return Err(last_os_error("macos.network-name"));
    }
    // SAFETY: successful `if_indextoname` wrote a NUL-terminated name into the buffer.
    Ok(unsafe { CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

fn read_uptime() -> Result<Duration, MetricsError> {
    let mut timebase = mach_timebase_info_data_t::default();
    // SAFETY: `timebase` points to writable storage for the Mach timebase ratio.
    let result = unsafe { mach_timebase_info(&mut timebase) };
    if result != KERN_SUCCESS || timebase.denom == 0 {
        return Err(MetricsError::new(
            "macos.uptime",
            format!("mach_timebase_info failed with {result}"),
        ));
    }
    // SAFETY: mach_continuous_time has no preconditions and includes suspended time.
    let ticks = u128::from(unsafe { mach_continuous_time() });
    let nanos = ticks
        .checked_mul(u128::from(timebase.numer))
        .ok_or_else(|| MetricsError::new("macos.uptime", "uptime counter overflowed"))?
        / u128::from(timebase.denom);
    let seconds = u64::try_from(nanos / 1_000_000_000)
        .map_err(|_| MetricsError::new("macos.uptime", "uptime seconds overflowed"))?;
    let subsecond_nanos = (nanos % 1_000_000_000) as u32;
    Ok(Duration::new(seconds, subsecond_nanos))
}

fn read_load() -> Result<LoadAverage, MetricsError> {
    let mut values = [0.0_f64; 3];
    // SAFETY: `values` points to writable storage for exactly three doubles.
    let count = unsafe { libc::getloadavg(values.as_mut_ptr(), values.len() as i32) };
    if count != values.len() as i32 {
        return Err(MetricsError::new(
            "macos.load",
            "getloadavg did not return all intervals",
        ));
    }
    Ok(LoadAverage {
        one_minute: values[0],
        five_minutes: values[1],
        fifteen_minutes: values[2],
    })
}

fn last_os_error(operation: &'static str) -> MetricsError {
    MetricsError::new(operation, std::io::Error::last_os_error().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_source_reports_supported_fields_without_fabricated_values() {
        let mut source = MacOsMetricsSource::new();
        let cancellation = CancellationToken::default();
        let raw = source.read(&cancellation).unwrap();
        let Reading::Available(cpu) = raw.cpu else {
            panic!("CPU metrics should be available");
        };
        assert!(cpu.total_ticks >= cpu.idle_ticks);
        let Reading::Available(memory) = raw.memory else {
            panic!("memory metrics should be available");
        };
        assert!(memory.total_bytes > 0);
        assert!(memory.available_bytes <= memory.total_bytes);
        let disks = read_disks(&cancellation).expect("disk metrics should be available");
        assert!(disks.iter().all(|disk| !disk.id.is_empty()));
        let Reading::Available(network) = raw.network else {
            panic!("network metrics should be available");
        };
        assert!(network.iter().all(|interface| !interface.id.is_empty()));
        assert!(matches!(raw.uptime, Reading::Available(value) if !value.is_zero()));
        assert!(matches!(raw.load_average, Reading::Available(_)));
        assert_eq!(raw.thermal_state, Reading::unsupported());
    }

    #[test]
    fn source_counters_are_cumulative_across_reads() {
        let mut source = MacOsMetricsSource::new();
        let cancellation = CancellationToken::default();
        let first = source.read(&cancellation).unwrap();
        let second = source.read(&cancellation).unwrap();
        let (Reading::Available(first_cpu), Reading::Available(second_cpu)) =
            (first.cpu, second.cpu)
        else {
            panic!("CPU metrics should be available");
        };
        assert!(second_cpu.total_ticks >= first_cpu.total_ticks);
    }

    #[test]
    fn cpu_state_indices_match_darwin_contract() {
        assert_eq!(CPU_STATE_USER, 0);
        assert_eq!(CPU_STATE_SYSTEM, 1);
        assert_eq!(CPU_STATE_IDLE, 2);
        assert_eq!(CPU_STATE_NICE, 3);
    }

    #[test]
    fn route_parser_rejects_truncated_tail_and_accepts_exact_unknown_message() {
        let cancellation = CancellationToken::default();
        assert!(parse_route_messages(&[1, 0, 0], &cancellation).is_err());
        assert_eq!(
            parse_route_messages(&[4, 0, 0, 0], &cancellation).unwrap(),
            Vec::new()
        );
    }
}
