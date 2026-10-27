import Darwin
import Foundation

actor SystemMetricsService {
    private let configuredVolumes: [PanelConfiguration.Volume]
    private var previousCPU: (used: UInt64, total: UInt64)?
    private var previousNetwork: (received: UInt64, sent: UInt64, date: Date)?

    init(volumes: [PanelConfiguration.Volume]) {
        configuredVolumes = volumes.isEmpty
            ? [.init(id: "system", label: "系统盘", path: "/")]
            : volumes
    }

    func sample() -> SystemMetrics {
        let cpu = cpuUsage()
        let memory = memoryUsage()
        let network = networkUsage()
        var loads = [Double](repeating: 0, count: 3)
        _ = getloadavg(&loads, 3)
        let volumes = configuredVolumes.map { volume in
            let usage = diskUsage(path: NSString(string: volume.path).expandingTildeInPath)
            return SystemMetrics.Volume(
                id: volume.id,
                label: volume.label,
                path: volume.path,
                percent: usage?.percent,
                freeBytes: usage?.free
            )
        }

        return SystemMetrics(
            cpuPercent: cpu,
            memoryPercent: memory.percent,
            memoryUsedBytes: memory.used,
            memoryTotalBytes: memory.total,
            volumes: volumes,
            networkDownBytesPerSecond: network.receivedPerSecond,
            networkUpBytesPerSecond: network.sentPerSecond,
            loadAverage: (loads[0], loads[1], loads[2]),
            uptimeSeconds: ProcessInfo.processInfo.systemUptime,
            thermalState: thermalState()
        )
    }

    private func cpuUsage() -> Double {
        var info = host_cpu_load_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<host_cpu_load_info_data_t>.stride / MemoryLayout<integer_t>.stride)
        let result = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics(mach_host_self(), HOST_CPU_LOAD_INFO, $0, &count)
            }
        }
        guard result == KERN_SUCCESS else { return 0 }

        let user = UInt64(info.cpu_ticks.0)
        let system = UInt64(info.cpu_ticks.1)
        let idle = UInt64(info.cpu_ticks.2)
        let nice = UInt64(info.cpu_ticks.3)
        let used = user + system + nice
        let total = used + idle
        defer { previousCPU = (used, total) }
        guard let previousCPU, total > previousCPU.total else { return 0 }
        let usedDelta = used - previousCPU.used
        let totalDelta = total - previousCPU.total
        return totalDelta > 0 ? Double(usedDelta) / Double(totalDelta) * 100 : 0
    }

    private func memoryUsage() -> (used: UInt64, total: UInt64, percent: Double) {
        var info = vm_statistics64_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<vm_statistics64_data_t>.stride / MemoryLayout<integer_t>.stride)
        let result = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics64(mach_host_self(), HOST_VM_INFO64, $0, &count)
            }
        }
        let total = ProcessInfo.processInfo.physicalMemory
        guard result == KERN_SUCCESS else { return (0, total, 0) }

        let pageSize = UInt64(getpagesize())
        let availablePages = UInt64(info.free_count + info.speculative_count)
        let available = min(total, availablePages * pageSize)
        let used = total - available
        return (used, total, total > 0 ? Double(used) / Double(total) * 100 : 0)
    }

    private func diskUsage(path: String) -> (free: UInt64, percent: Double)? {
        guard
            let attributes = try? FileManager.default.attributesOfFileSystem(forPath: path),
            let total = (attributes[.systemSize] as? NSNumber)?.uint64Value,
            let free = (attributes[.systemFreeSize] as? NSNumber)?.uint64Value,
            total > 0
        else { return nil }
        return (free, Double(total - free) / Double(total) * 100)
    }

    private func networkUsage() -> (receivedPerSecond: Double, sentPerSecond: Double) {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else { return (0, 0) }
        defer { freeifaddrs(head) }

        var received: UInt64 = 0
        var sent: UInt64 = 0
        var cursor: UnsafeMutablePointer<ifaddrs>? = first
        while let interface = cursor {
            let item = interface.pointee
            if let address = item.ifa_addr,
               address.pointee.sa_family == UInt8(AF_LINK),
               (item.ifa_flags & UInt32(IFF_UP)) != 0,
               (item.ifa_flags & UInt32(IFF_LOOPBACK)) == 0,
               let raw = item.ifa_data {
                let data = raw.assumingMemoryBound(to: if_data.self).pointee
                received += UInt64(data.ifi_ibytes)
                sent += UInt64(data.ifi_obytes)
            }
            cursor = item.ifa_next
        }

        let now = Date()
        defer { previousNetwork = (received, sent, now) }
        guard let previousNetwork else { return (0, 0) }
        let interval = now.timeIntervalSince(previousNetwork.date)
        guard interval > 0 else { return (0, 0) }
        let receivedDelta = received >= previousNetwork.received ? received - previousNetwork.received : 0
        let sentDelta = sent >= previousNetwork.sent ? sent - previousNetwork.sent : 0
        return (Double(receivedDelta) / interval, Double(sentDelta) / interval)
    }

    private func thermalState() -> String {
        switch ProcessInfo.processInfo.thermalState {
        case .nominal: return "正常"
        case .fair: return "偏热"
        case .serious: return "较热"
        case .critical: return "过热"
        @unknown default: return "未知"
        }
    }
}
