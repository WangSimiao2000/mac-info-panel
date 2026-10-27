import Foundation
import SwiftUI

struct LyricSnapshot: Sendable {
    var available = false
    var playing = false
    var title = "等待播放器"
    var artists = "请启动 OmniLyrics 和受支持的音乐播放器"
    var currentLine = "暂无歌词"
    var currentTranslation = ""
    var nextLine = ""
    var progress: Double = 0
    var error: String?
}

struct SystemMetrics: Sendable {
    var cpuPercent: Double = 0
    var memoryPercent: Double = 0
    var memoryUsedBytes: UInt64 = 0
    var memoryTotalBytes: UInt64 = 0
    var rootDiskPercent: Double = 0
    var rootDiskFreeBytes: UInt64 = 0
    var externalDiskPercent: Double?
    var externalDiskFreeBytes: UInt64?
    var networkDownBytesPerSecond: Double = 0
    var networkUpBytesPerSecond: Double = 0
    var loadAverage: (Double, Double, Double) = (0, 0, 0)
    var uptimeSeconds: TimeInterval = 0
    var thermalState = "正常"
}

struct MCPanelSnapshot: Decodable, Sendable {
    struct Agent: Decodable, Sendable {
        var health: String?
        var version: String?
        var serviceActive: Bool?
    }

    struct Minecraft: Decodable, Sendable {
        var state: String?
        var serviceActive: Bool?
        var online: Bool?
        var playersOnline: Int?
        var playersMax: Int?
        var version: String?
        var uptimeSeconds: Double?
    }

    struct Host: Decodable, Sendable {
        var cpuPercent: Double?
        var memoryPercent: Double?
        var diskPercent: Double?
        var load1: Double?
        var uptimeSeconds: Double?
    }

    var schema: Int
    var generatedAt: String?
    var agent: Agent?
    var minecraft: Minecraft?
    var host: Host?
    var warning: String?
}

@MainActor
final class DashboardModel: ObservableObject {
    @Published var now = Date()
    @Published var lyrics = LyricSnapshot()
    @Published var system = SystemMetrics()
    @Published var minecraft: MCPanelSnapshot?
    @Published var minecraftError: String?

    private let configuration: PanelConfiguration
    private let lyricsClient: OmniLyricsClient
    private let metricsService = SystemMetricsService()
    private let mcService: MCPanelService
    private var tasks: [Task<Void, Never>] = []

    init(configuration: PanelConfiguration) {
        self.configuration = configuration
        lyricsClient = OmniLyricsClient(url: configuration.lyricsURL)
        mcService = MCPanelService(configuration: configuration)
    }

    func start() {
        guard tasks.isEmpty else { return }

        tasks.append(Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                now = Date()
                lyrics = await lyricsClient.fetch()
                await sleep(seconds: configuration.lyricsRefreshSeconds)
            }
        })

        tasks.append(Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                system = await metricsService.sample()
                await sleep(seconds: configuration.systemRefreshSeconds)
            }
        })

        tasks.append(Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                do {
                    minecraft = try await mcService.fetch()
                    minecraftError = nil
                } catch {
                    minecraftError = error.localizedDescription
                }
                await sleep(seconds: configuration.remoteRefreshSeconds)
            }
        })
    }

    func stop() {
        tasks.forEach { $0.cancel() }
        tasks.removeAll()
    }

    private func sleep(seconds: Double) async {
        let nanoseconds = UInt64(max(0.1, seconds) * 1_000_000_000)
        try? await Task.sleep(nanoseconds: nanoseconds)
    }
}
