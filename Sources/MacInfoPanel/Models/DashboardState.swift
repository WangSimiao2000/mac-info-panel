import Foundation
import SwiftUI

struct LyricToken: Sendable, Identifiable {
    let id: Int
    let startTime: Double
    let duration: Double
    let text: String
}

struct LyricLine: Sendable, Identifiable {
    let id: Int
    let startTime: Double
    let endTime: Double?
    let text: String
    let translation: String
    let tokens: [LyricToken]
}

struct LyricPresentation: Sendable {
    let previous: String
    let current: String
    let translation: String
    let next: String
    let tokens: [LyricToken]
    let position: Double
    let progress: Double
}

struct LyricSnapshot: Sendable {
    var apiAvailable = false
    var playerAvailable = false
    var loading = false
    var playing = false
    var title = "等待播放器"
    var artists = "打开支持系统“正在播放”的音乐应用"
    var positionSeconds: Double = 0
    var durationSeconds: Double = 0
    var sampledAt = Date()
    var lyricsVersion = ""
    var lines: [LyricLine] = []
    var error: String?

    func estimatedPosition(at date: Date) -> Double {
        let elapsed = playing ? max(0, date.timeIntervalSince(sampledAt)) : 0
        return min(durationSeconds > 0 ? durationSeconds : .greatestFiniteMagnitude, max(0, positionSeconds + elapsed))
    }

    func presentation(at date: Date) -> LyricPresentation {
        let position = estimatedPosition(at: date)
        let index = lines.lastIndex { $0.startTime <= position }
        let current = index.map { lines[$0] }
        let previous = index.flatMap { $0 > 0 ? lines[$0 - 1].text : nil } ?? ""
        let next = index.flatMap { $0 + 1 < lines.count ? lines[$0 + 1].text : nil }
            ?? (index == nil ? lines.first?.text : nil)
            ?? ""
        let message: String
        if !apiAvailable { message = "等待 OmniLyrics" }
        else if !playerAvailable { message = "等待播放器" }
        else if loading { message = "正在匹配歌词…" }
        else if lines.isEmpty { message = "暂未匹配到歌词" }
        else { message = current?.text ?? "♪" }
        return LyricPresentation(
            previous: previous,
            current: message,
            translation: current?.translation ?? "",
            next: next,
            tokens: current?.tokens ?? [],
            position: position,
            progress: durationSeconds > 0 ? min(1, max(0, position / durationSeconds)) : 0
        )
    }
}

struct SystemMetrics: Sendable {
    struct Volume: Identifiable, Sendable {
        let id: String
        let label: String
        let path: String
        let percent: Double?
        let freeBytes: UInt64?
    }

    var cpuPercent: Double = 0
    var memoryPercent: Double = 0
    var memoryUsedBytes: UInt64 = 0
    var memoryTotalBytes: UInt64 = 0
    var volumes: [Volume] = []
    var networkDownBytesPerSecond: Double = 0
    var networkUpBytesPerSecond: Double = 0
    var loadAverage: (Double, Double, Double) = (0, 0, 0)
    var uptimeSeconds: TimeInterval = 0
    var thermalState = "正常"
}

struct DataSourceState: Sendable {
    var snapshot: HTTPJSONSnapshot?
    var lastSuccess: Date?
    var error: String?
    var stale = false
}

@MainActor
final class DashboardModel: ObservableObject {
    @Published var now = Date()
    @Published var lyrics = LyricSnapshot()
    @Published var system = SystemMetrics()
    @Published var sourceStates: [String: DataSourceState] = [:]
    @Published var selectedPageIndex = 0
    @Published var autoRotateEnabled: Bool

    let configuration: PanelConfiguration
    let configurationWarning: String?
    private let lyricsClient: OmniLyricsClient
    private let metricsService: SystemMetricsService
    private var tasks: [Task<Void, Never>] = []

    init(configuration: PanelConfiguration, configurationWarning: String?) {
        self.configuration = configuration
        self.configurationWarning = configurationWarning
        autoRotateEnabled = configuration.behavior.autoRotateSeconds > 0
        lyricsClient = OmniLyricsClient(url: configuration.lyrics.url)
        metricsService = SystemMetricsService(volumes: configuration.volumes)
        selectedPageIndex = configuration.pages.firstIndex { $0.id == configuration.behavior.initialPage } ?? 0
        for source in configuration.sources { sourceStates[source.id] = DataSourceState() }
    }

    var selectedPage: PanelConfiguration.Page {
        configuration.pages.indices.contains(selectedPageIndex) ? configuration.pages[selectedPageIndex] : configuration.pages[0]
    }

    func start() {
        guard tasks.isEmpty else { return }

        if configuration.lyrics.enabled {
            tasks.append(Task { [weak self] in
                while !Task.isCancelled {
                    guard let self else { return }
                    now = Date()
                    lyrics = await lyricsClient.fetch()
                    let interval = lyrics.playing ? configuration.lyrics.refreshSeconds : max(3, configuration.lyrics.refreshSeconds)
                    await sleep(seconds: interval)
                }
            })
        }

        tasks.append(Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                now = Date()
                system = await metricsService.sample()
                await sleep(seconds: 3)
            }
        })

        for source in configuration.sources {
            let client = HTTPJSONClient(source: source)
            tasks.append(Task { [weak self] in
                while !Task.isCancelled {
                    guard let self else { return }
                    do {
                        let snapshot = try await client.fetch()
                        sourceStates[source.id] = DataSourceState(
                            snapshot: snapshot,
                            lastSuccess: snapshot.receivedAt,
                            error: nil,
                            stale: false
                        )
                    } catch {
                        var state = sourceStates[source.id] ?? DataSourceState()
                        state.error = error.localizedDescription
                        if let lastSuccess = state.lastSuccess {
                            state.stale = Date().timeIntervalSince(lastSuccess) > source.staleAfterSeconds
                        }
                        sourceStates[source.id] = state
                    }
                    await sleep(seconds: source.refreshSeconds)
                }
            })
        }

        tasks.append(Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                let interval = max(1, configuration.behavior.autoRotateSeconds)
                await sleep(seconds: interval)
                if autoRotateEnabled { nextPage() }
            }
        })
    }

    func stop() {
        tasks.forEach { $0.cancel() }
        tasks.removeAll()
    }

    func selectPage(at index: Int) {
        guard configuration.pages.indices.contains(index) else { return }
        selectedPageIndex = index
    }

    func nextPage() {
        selectedPageIndex = (selectedPageIndex + 1) % configuration.pages.count
    }

    func previousPage() {
        selectedPageIndex = (selectedPageIndex - 1 + configuration.pages.count) % configuration.pages.count
    }

    func toggleAutoRotate() {
        autoRotateEnabled.toggle()
    }

    private func sleep(seconds: Double) async {
        let nanoseconds = UInt64(max(0.1, seconds) * 1_000_000_000)
        try? await Task.sleep(nanoseconds: nanoseconds)
    }
}
