use std::mem::{MaybeUninit, size_of};
use std::ptr;
use std::time::Duration;

use panel_metrics::{
    CancellationToken, CpuCounters, MetricSupport, MetricsCapabilities, MetricsError,
    MetricsSource, NetworkCounters, RawDisk, RawMemory, RawMetrics, Reading, UnavailableReason,
};
use windows_sys::Win32::Foundation::{
    ERROR_MORE_DATA, ERROR_NO_MORE_FILES, ERROR_SUCCESS, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows_sys::Win32::Storage::FileSystem::{
    FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW,
    GetVolumePathNamesForVolumeNameW,
};
use windows_sys::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows_sys::Win32::System::Threading::GetSystemTimes;
use windows_sys::core::GUID;

#[derive(Debug, Default)]
pub struct WindowsMetricsSource;

impl WindowsMetricsSource {
    pub const fn new() -> Self {
        Self
    }
}

impl MetricsSource for WindowsMetricsSource {
    fn capabilities(&self) -> MetricsCapabilities {
        MetricsCapabilities {
            cpu: MetricSupport::Available,
            memory: MetricSupport::Available,
            disks: MetricSupport::Available,
            network: MetricSupport::Available,
            uptime: MetricSupport::Available,
            load_average: MetricSupport::Unsupported,
            thermal_state: MetricSupport::Unsupported,
        }
    }

    fn read(&mut self, cancellation: &CancellationToken) -> Result<RawMetrics, MetricsError> {
        cancellation.check()?;
        let cpu = reading(read_cpu());
        cancellation.check()?;
        let memory = reading(read_memory());
        cancellation.check()?;
        let disks = reading(read_disks(cancellation));
        cancellation.check()?;
        let network = reading(read_network(cancellation));
        cancellation.check()?;
        let uptime = Reading::Available(read_uptime());
        cancellation.check()?;
        Ok(RawMetrics {
            cpu,
            memory,
            disks,
            network,
            uptime,
            load_average: Reading::unsupported(),
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

fn read_cpu() -> Result<CpuCounters, MetricsError> {
    let mut idle = MaybeUninit::<FILETIME>::zeroed();
    let mut kernel = MaybeUninit::<FILETIME>::zeroed();
    let mut user = MaybeUninit::<FILETIME>::zeroed();
    // SAFETY: all three pointers refer to writable FILETIME storage.
    if unsafe { GetSystemTimes(idle.as_mut_ptr(), kernel.as_mut_ptr(), user.as_mut_ptr()) } == 0 {
        return Err(last_os_error("windows.cpu"));
    }
    // SAFETY: successful GetSystemTimes initialized all three values.
    let (idle, kernel, user) =
        unsafe { (idle.assume_init(), kernel.assume_init(), user.assume_init()) };
    let kernel_ticks = filetime_ticks(kernel);
    let user_ticks = filetime_ticks(user);
    let total_ticks = kernel_ticks
        .checked_add(user_ticks)
        .ok_or_else(|| MetricsError::new("windows.cpu", "CPU counters overflowed"))?;
    Ok(CpuCounters {
        total_ticks,
        idle_ticks: filetime_ticks(idle),
    })
}

fn filetime_ticks(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn read_memory() -> Result<RawMemory, MetricsError> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        dwMemoryLoad: 0,
        ullTotalPhys: 0,
        ullAvailPhys: 0,
        ullTotalPageFile: 0,
        ullAvailPageFile: 0,
        ullTotalVirtual: 0,
        ullAvailVirtual: 0,
        ullAvailExtendedVirtual: 0,
    };
    // SAFETY: `status` has the documented length and points to writable storage.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(last_os_error("windows.memory"));
    }
    Ok(RawMemory {
        total_bytes: status.ullTotalPhys,
        available_bytes: status.ullAvailPhys,
    })
}

struct VolumeSearch(HANDLE);

impl Drop for VolumeSearch {
    fn drop(&mut self) {
        // SAFETY: this guard owns the valid search handle returned by FindFirstVolumeW.
        unsafe {
            FindVolumeClose(self.0);
        }
    }
}

fn read_disks(cancellation: &CancellationToken) -> Result<Vec<RawDisk>, MetricsError> {
    const VOLUME_NAME_UNITS: usize = 260;
    let mut volume_buffer = [0_u16; VOLUME_NAME_UNITS];
    cancellation.check()?;
    // SAFETY: volume_buffer is writable for the exact length supplied.
    let handle = unsafe {
        FindFirstVolumeW(
            volume_buffer.as_mut_ptr(),
            u32::try_from(volume_buffer.len())
                .map_err(|_| MetricsError::new("windows.disk", "volume buffer is too large"))?,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last_os_error("windows.disk-list"));
    }
    let search = VolumeSearch(handle);
    let mut disks = Vec::new();
    let mut enumerated = 0_usize;
    loop {
        cancellation.check()?;
        let volume_name = nul_terminated(&volume_buffer)
            .ok_or_else(|| MetricsError::new("windows.disk", "volume name is not terminated"))?;
        if volume_name.len() <= 1 {
            return Err(MetricsError::new("windows.disk", "volume name is empty"));
        }
        enumerated = enumerated
            .checked_add(1)
            .ok_or_else(|| MetricsError::new("windows.disk", "volume count overflowed"))?;
        let paths = volume_paths(volume_name)?;
        cancellation.check()?;
        let mut available_bytes = 0_u64;
        let mut total_bytes = 0_u64;
        let mut total_free_bytes = 0_u64;
        // Volume GUID paths enumerate local volumes and avoid probing disconnected remote drives.
        // SAFETY: volume_name is NUL-terminated and all output pointers are writable u64 values.
        if unsafe {
            GetDiskFreeSpaceExW(
                volume_name.as_ptr(),
                &mut available_bytes,
                &mut total_bytes,
                &mut total_free_bytes,
            )
        } != 0
        {
            let stable_name = utf16_nul(volume_name);
            // With multiple mount points the lexicographically first path is the deterministic
            // display path. An unmounted volume uses an empty mount point and its GUID as its name.
            let (name, mount_point) = volume_display(&stable_name, &paths);
            disks.push(RawDisk {
                id: format!("windows:volume:{}", stable_name.to_lowercase()),
                name,
                mount_point,
                total_bytes,
                available_bytes,
            });
        }
        cancellation.check()?;
        volume_buffer.fill(0);
        // SAFETY: search owns a valid handle and volume_buffer is writable for its supplied length.
        if unsafe {
            FindNextVolumeW(
                search.0,
                volume_buffer.as_mut_ptr(),
                u32::try_from(volume_buffer.len())
                    .map_err(|_| MetricsError::new("windows.disk", "volume buffer is too large"))?,
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                break;
            }
            return Err(MetricsError::new("windows.disk-list", error.to_string()));
        }
    }
    cancellation.check()?;
    if enumerated > 0 && disks.is_empty() {
        return Err(MetricsError::new(
            "windows.disk",
            "no enumerated local volume exposed capacity data",
        ));
    }
    Ok(disks)
}

fn volume_paths(volume_name: &[u16]) -> Result<Vec<String>, MetricsError> {
    const MAX_PATH_UNITS: u32 = 32 * 1024;
    let mut required = 0_u32;
    // SAFETY: volume_name is NUL-terminated; this query only writes the required length.
    let result = unsafe {
        GetVolumePathNamesForVolumeNameW(volume_name.as_ptr(), ptr::null_mut(), 0, &mut required)
    };
    if result == 0 && std::io::Error::last_os_error().raw_os_error() != Some(ERROR_MORE_DATA as i32)
    {
        return Err(last_os_error("windows.volume-paths"));
    }
    if required == 0 {
        return Ok(Vec::new());
    }
    if required > MAX_PATH_UNITS {
        return Err(MetricsError::new(
            "windows.volume-paths",
            "volume path list exceeds its size limit",
        ));
    }
    let mut buffer = vec![0_u16; required as usize];
    // SAFETY: buffer is writable for required UTF-16 units and volume_name is NUL-terminated.
    if unsafe {
        GetVolumePathNamesForVolumeNameW(
            volume_name.as_ptr(),
            buffer.as_mut_ptr(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(last_os_error("windows.volume-paths"));
    }
    let mut paths = multi_string(&buffer)
        .into_iter()
        .map(utf16_nul)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn nul_terminated(value: &[u16]) -> Option<&[u16]> {
    value
        .iter()
        .position(|unit| *unit == 0)
        .map(|end| &value[..=end])
}

fn multi_string(value: &[u16]) -> Vec<&[u16]> {
    let mut strings = Vec::new();
    let mut start = 0;
    for (index, unit) in value.iter().enumerate() {
        if *unit == 0 {
            if index > start {
                strings.push(&value[start..=index]);
            }
            start = index + 1;
        }
    }
    strings
}

fn volume_display(stable_name: &str, sorted_paths: &[String]) -> (String, String) {
    let mount_point = sorted_paths.first().cloned().unwrap_or_default();
    let name = if mount_point.is_empty() {
        stable_name.to_owned()
    } else {
        mount_point.clone()
    };
    (name, mount_point)
}

fn utf16_nul(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

struct MibTable(*mut MIB_IF_TABLE2);

impl Drop for MibTable {
    fn drop(&mut self) {
        // SAFETY: GetIfTable2 allocated this pointer and this guard frees it exactly once.
        unsafe { FreeMibTable(self.0.cast()) };
    }
}

fn read_network(cancellation: &CancellationToken) -> Result<Vec<NetworkCounters>, MetricsError> {
    cancellation.check()?;
    let mut table = ptr::null_mut();
    // SAFETY: `table` points to writable pointer storage as required by GetIfTable2.
    let result = unsafe { GetIfTable2(&mut table) };
    if result != ERROR_SUCCESS || table.is_null() {
        return Err(MetricsError::new(
            "windows.network",
            format!("GetIfTable2 failed with {result}"),
        ));
    }
    let table = MibTable(table);
    // SAFETY: GetIfTable2 returned a valid table pointer retained by the guard.
    let count = unsafe { (*table.0).NumEntries as usize };
    if count > 65_536 {
        return Err(MetricsError::new(
            "windows.network",
            "interface count exceeds its size limit",
        ));
    }
    // SAFETY: the variable-length Table member contains exactly NumEntries rows by API contract.
    let rows = unsafe {
        let first = (*table.0).Table.as_ptr();
        std::slice::from_raw_parts(first, count)
    };
    let mut interfaces = Vec::with_capacity(rows.len());
    for row in rows {
        cancellation.check()?;
        interfaces.push(network_counter(row));
    }
    cancellation.check()?;
    Ok(interfaces)
}

fn network_counter(row: &MIB_IF_ROW2) -> NetworkCounters {
    let name = {
        let alias = utf16_nul(&row.Alias);
        if alias.is_empty() {
            utf16_nul(&row.Description)
        } else {
            alias
        }
    };
    let id = if guid_is_zero(row.InterfaceGuid) {
        format!("windows:ifindex:{}", row.InterfaceIndex)
    } else {
        format!("windows:interface:{}", guid(row.InterfaceGuid))
    };
    NetworkCounters {
        id,
        name,
        received_bytes: row.InOctets,
        transmitted_bytes: row.OutOctets,
    }
}

fn guid(value: GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        value.data1,
        value.data2,
        value.data3,
        value.data4[0],
        value.data4[1],
        value.data4[2],
        value.data4[3],
        value.data4[4],
        value.data4[5],
        value.data4[6],
        value.data4[7]
    )
}

fn guid_is_zero(value: GUID) -> bool {
    value.data1 == 0
        && value.data2 == 0
        && value.data3 == 0
        && value.data4.iter().all(|byte| *byte == 0)
}

fn read_uptime() -> Duration {
    // SAFETY: GetTickCount64 has no preconditions and returns milliseconds since boot.
    Duration::from_millis(unsafe { GetTickCount64() })
}

fn last_os_error(operation: &'static str) -> MetricsError {
    MetricsError::new(operation, std::io::Error::last_os_error().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_source_reports_capabilities_and_never_fabricates_unavailable_values() {
        let mut source = WindowsMetricsSource::new();
        let capabilities = source.capabilities();
        assert_eq!(capabilities.cpu, MetricSupport::Available);
        assert_eq!(capabilities.memory, MetricSupport::Available);
        assert_eq!(capabilities.disks, MetricSupport::Available);
        assert_eq!(capabilities.network, MetricSupport::Available);
        assert_eq!(capabilities.uptime, MetricSupport::Available);
        assert_eq!(capabilities.load_average, MetricSupport::Unsupported);

        let raw = source.read(&CancellationToken::default()).unwrap();
        assert_reading(&raw.cpu, |cpu| {
            cpu.total_ticks > 0 && cpu.idle_ticks <= cpu.total_ticks
        });
        assert_reading(&raw.memory, |memory| {
            memory.total_bytes > 0 && memory.available_bytes <= memory.total_bytes
        });
        assert_reading(&raw.disks, |disks| {
            disks.iter().all(|disk| {
                !disk.id.is_empty()
                    && disk.total_bytes > 0
                    && disk.available_bytes <= disk.total_bytes
            })
        });
        assert_reading(&raw.network, |interfaces| {
            interfaces.iter().all(|interface| !interface.id.is_empty())
        });
        assert_reading(&raw.uptime, |uptime| !uptime.is_zero());
        assert_eq!(raw.load_average, Reading::unsupported());
        assert_eq!(raw.thermal_state, Reading::unsupported());
    }

    fn assert_reading<T>(reading: &Reading<T>, invariant: impl FnOnce(&T) -> bool) {
        match reading {
            Reading::Available(value) => assert!(invariant(value)),
            Reading::Unavailable(reason) => {
                assert_eq!(*reason, UnavailableReason::TemporarilyUnavailable);
            }
        }
    }

    #[test]
    fn volume_display_handles_unmounted_and_multiple_mount_points() {
        assert_eq!(
            volume_display(r"\\?\Volume{stable}\", &[]),
            (r"\\?\Volume{stable}\".into(), String::new())
        );
        assert_eq!(
            volume_display(
                r"\\?\Volume{stable}\",
                &[r"C:\".into(), r"C:\mount\data\".into()]
            ),
            (r"C:\".into(), r"C:\".into())
        );
    }

    #[test]
    fn guid_format_is_stable_and_canonical() {
        assert_eq!(
            guid(GUID {
                data1: 0x1234_5678,
                data2: 0xabcd,
                data3: 0xef01,
                data4: [0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01],
            }),
            "12345678-abcd-ef01-2345-6789abcdef01"
        );
    }
}
