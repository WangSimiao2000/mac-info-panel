import SwiftUI

private let accent = Color.accentColor
private let windowShape = RoundedRectangle(cornerRadius: 30, style: .continuous)

struct DashboardView: View {
    @ObservedObject var model: DashboardModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 12) {
            topBar
            pageContent
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .id(model.selectedPage.id)
                .transition(.opacity.combined(with: .scale(scale: 0.985)))
                .animation(reduceMotion ? nil : .smooth(duration: 0.26), value: model.selectedPageIndex)
            pageIndicator
        }
        .padding(14)
        .foregroundStyle(.primary)
        .windowGlass()
        .clipShape(windowShape)
        .overlay(windowShape.stroke(Color.primary.opacity(0.12), lineWidth: 1))
        .shadow(color: .black.opacity(0.28), radius: 20, y: 8)
        .padding(2)
    }

    private var topBar: some View {
        HStack(spacing: 12) {
            Image(systemName: pageSymbol)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(accent)
            Text(model.selectedPage.title)
                .font(.system(size: 19, weight: .semibold))
            if let warning = model.configurationWarning {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .help(warning)
            }
            Spacer()
            Text(model.now, format: .dateTime.month().day().weekday(.abbreviated))
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Color.primary.opacity(0.56))
            Text(model.now, format: .dateTime.hour().minute())
                .font(.system(size: 25, weight: .semibold))
                .monospacedDigit()
        }
        .padding(.horizontal, 16)
        .frame(height: 48)
        .controlGlass(cornerRadius: 18)
    }

    @ViewBuilder
    private var pageContent: some View {
        switch model.selectedPage.kind {
        case .overview: overviewPage
        case .lyrics: lyricsPage
        case .httpJSON: sourcePage(sourceID: model.selectedPage.sourceID)
        case .localSystem: localSystemPage
        }
    }

    private var overviewPage: some View {
        GeometryReader { geometry in
            HStack(spacing: 12) {
                TimelineView(.periodic(from: .now, by: 1.0)) { timeline in
                    NowPlayingOverview(
                        snapshot: model.lyrics,
                        date: timeline.date,
                        fontDesign: lyricFontDesign,
                        fontScale: lyricFontScale
                    )
                }
                .frame(width: geometry.size.width * 0.59)

                VStack(spacing: 12) {
                    if let source = model.configuration.sources.first {
                        sourceSummary(source)
                    } else {
                        ContentSurface {
                            EmptyState(symbol: "network", title: "没有数据源", detail: "在 config.json 中添加 HTTPS JSON 接口")
                        }
                    }
                    LocalSummary(metrics: model.system)
                }
            }
        }
    }

    private var lyricsPage: some View {
        TimelineView(.periodic(from: .now, by: model.lyrics.lines.contains(where: { !$0.tokens.isEmpty }) ? 0.25 : 1.0)) { timeline in
            let presentation = model.lyrics.presentation(at: timeline.date)
            ContentSurface(padding: 24) {
                VStack(spacing: 0) {
                    HStack(spacing: 12) {
                        Image(systemName: model.lyrics.playing ? "waveform" : "pause.fill")
                            .foregroundStyle(accent)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(model.lyrics.title)
                                .font(.system(size: 21, weight: .semibold))
                                .lineLimit(1)
                            Text(model.lyrics.artists)
                                .font(.system(size: 15, weight: .medium))
                                .foregroundStyle(Color.primary.opacity(0.52))
                                .lineLimit(1)
                        }
                        Spacer()
                        Text(time(presentation.position))
                            .font(.system(size: 15, weight: .medium, design: .monospaced))
                            .foregroundStyle(Color.primary.opacity(0.52))
                    }

                    Spacer(minLength: 8)
                    Text(presentation.previous)
                        .font(.system(size: 19, weight: .medium))
                        .foregroundStyle(Color.primary.opacity(0.45))
                        .lineLimit(1)

                    Spacer(minLength: 14)
                    karaokeText(presentation)
                        .font(.system(size: 45 * lyricFontScale, weight: .semibold, design: lyricFontDesign))
                        .multilineTextAlignment(.center)
                        .lineSpacing(5)
                        .lineLimit(3)
                        .minimumScaleFactor(0.62)
                        .frame(maxWidth: .infinity)

                    if !presentation.translation.isEmpty {
                        Text(presentation.translation)
                            .font(.system(size: 23 * lyricFontScale, weight: .medium, design: lyricFontDesign))
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                            .lineLimit(2)
                            .padding(.top, 10)
                    }

                    Spacer(minLength: 14)
                    Text(presentation.next)
                        .font(.system(size: 21, weight: .medium))
                        .foregroundStyle(Color.primary.opacity(0.60))
                        .lineLimit(1)
                    Spacer(minLength: 12)
                    ProgressView(value: presentation.progress)
                        .tint(accent)
                }
            }
        }
    }

    @ViewBuilder
    private func sourcePage(sourceID: String?) -> some View {
        if let sourceID, let source = model.configuration.sources.first(where: { $0.id == sourceID }) {
            let state = model.sourceStates[sourceID]
            VStack(spacing: 12) {
                HStack {
                    Label(source.title, systemImage: source.symbol)
                        .font(.system(size: 25, weight: .bold))
                    Spacer()
                    SourceHealth(state: state, source: source)
                }
                .padding(.horizontal, 5)

                if let fields = state?.snapshot?.fields, !fields.isEmpty {
                    LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 12), count: 3), spacing: 12) {
                        ForEach(fields) { field in SourceFieldView(field: field) }
                    }
                    .frame(maxHeight: .infinity)
                } else {
                    ContentSurface {
                        EmptyState(symbol: "network.slash", title: "等待数据", detail: state?.error ?? source.url.host ?? source.url.absoluteString)
                    }
                }
            }
        } else {
            ContentSurface {
                EmptyState(symbol: "exclamationmark.triangle", title: "配置错误", detail: "页面没有对应的数据源")
            }
        }
    }

    private var localSystemPage: some View {
        GeometryReader { geometry in
            HStack(spacing: 12) {
                ContentSurface {
                    HStack(spacing: 28) {
                        MetricRing(label: "CPU", value: model.system.cpuPercent, color: accent)
                        MetricRing(label: "内存", value: model.system.memoryPercent, color: .purple)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                .frame(width: geometry.size.width * 0.43)

                VStack(spacing: 12) {
                    ContentSurface {
                        HStack(spacing: 24) {
                            CompactMetric(label: "系统负载", value: String(format: "%.2f", model.system.loadAverage.0), detail: "5m  \(String(format: "%.2f", model.system.loadAverage.1))")
                            Divider().overlay(Color.primary.opacity(0.08))
                            CompactMetric(label: "热状态", value: model.system.thermalState, detail: "无需管理员权限")
                            Divider().overlay(Color.primary.opacity(0.08))
                            CompactMetric(label: "运行时间", value: duration(model.system.uptimeSeconds), detail: "macOS")
                        }
                    }
                    ContentSurface {
                        HStack(spacing: 18) {
                            ForEach(model.system.volumes.prefix(2)) { volume in
                                CompactMetric(label: volume.label, value: volume.freeBytes.map(bytes) ?? "未挂载", detail: "可用空间")
                            }
                            Spacer(minLength: 0)
                            CompactMetric(label: "网络下载", value: rate(model.system.networkDownBytesPerSecond), detail: "上传 \(rate(model.system.networkUpBytesPerSecond))")
                        }
                    }
                }
            }
        }
    }

    private func sourceSummary(_ source: PanelConfiguration.HTTPJSONSource) -> some View {
        let state = model.sourceStates[source.id]
        return ContentSurface {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Label(source.title, systemImage: source.symbol)
                        .font(.system(size: 18, weight: .semibold))
                    Spacer()
                    SourceHealth(state: state, source: source, compact: true)
                }
                if let fields = state?.snapshot?.fields {
                    ForEach(Array(fields.prefix(3))) { field in
                        HStack {
                            Text(field.label).foregroundStyle(Color.primary.opacity(0.50))
                            Spacer()
                            if let positive = field.positive { StatusDot(good: positive) }
                            Text(field.text).fontWeight(.semibold).monospacedDigit().lineLimit(1)
                        }
                        .font(.system(size: 16))
                    }
                } else {
                    EmptyState(symbol: "arrow.clockwise", title: "正在加载", detail: state?.error ?? "")
                }
            }
        }
    }

    private var pageIndicator: some View {
        HStack(spacing: 8) {
            if model.autoRotateEnabled {
                Image(systemName: "arrow.triangle.2.circlepath")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(accent)
            }
            ForEach(Array(model.configuration.pages.enumerated()), id: \.element.id) { index, page in
                Capsule()
                    .fill(index == model.selectedPageIndex ? Color.primary : Color.primary.opacity(0.22))
                    .frame(width: index == model.selectedPageIndex ? 20 : 6, height: 6)
                    .accessibilityLabel(page.title)
            }
        }
        .padding(.horizontal, 13)
        .frame(height: 24)
        .controlGlass(cornerRadius: 12)
        .frame(maxWidth: .infinity, alignment: .center)
    }

    private var pageSymbol: String {
        switch model.selectedPage.kind {
        case .overview: "rectangle.grid.2x2"
        case .lyrics: "quote.bubble"
        case .httpJSON: "server.rack"
        case .localSystem: "macmini"
        }
    }

    private func karaokeText(_ presentation: LyricPresentation) -> Text {
        guard !presentation.tokens.isEmpty else { return Text(presentation.current) }
        return presentation.tokens.reduce(Text("")) { result, token in
            let active = presentation.position >= token.startTime
            return result + Text(token.text).foregroundColor(active ? .primary : .secondary.opacity(0.72))
        }
    }

    private var lyricFontScale: Double {
        model.configuration.appearance?.lyricFontScale ?? 1
    }

    private var lyricFontDesign: Font.Design {
        switch model.configuration.appearance?.lyricFontDesign ?? .system {
        case .system: return .default
        case .rounded: return .rounded
        case .serif: return .serif
        case .monospaced: return .monospaced
        }
    }

    private func duration(_ seconds: Double) -> String {
        let minutes = max(0, Int(seconds)) / 60
        let days = minutes / 1_440
        let hours = (minutes % 1_440) / 60
        if days > 0 { return "\(days)天 \(hours)小时" }
        if hours > 0 { return "\(hours)小时" }
        return "\(minutes)分钟"
    }

    private func time(_ seconds: Double) -> String {
        String(format: "%d:%02d", max(0, Int(seconds)) / 60, max(0, Int(seconds)) % 60)
    }

    private func bytes(_ value: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .file)
    }

    private func rate(_ value: Double) -> String {
        "\(ByteCountFormatter.string(fromByteCount: Int64(max(0, value)), countStyle: .file))/s"
    }
}

private struct NowPlayingOverview: View {
    let snapshot: LyricSnapshot
    let date: Date
    let fontDesign: Font.Design
    let fontScale: Double

    var body: some View {
        let presentation = snapshot.presentation(at: date)
        ContentSurface(padding: 24) {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 9) {
                    Image(systemName: snapshot.playing ? "waveform" : "music.note")
                        .foregroundStyle(accent)
                    Text("正在播放")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(Color.primary.opacity(0.55))
                    Spacer()
                    Text(snapshot.playing ? "播放中" : "已暂停")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color.primary.opacity(0.42))
                }
                Text(snapshot.title)
                    .font(.system(size: 28, weight: .bold))
                    .lineLimit(1)
                    .padding(.top, 16)
                Text(snapshot.artists)
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(Color.primary.opacity(0.48))
                    .lineLimit(1)
                    .padding(.top, 4)
                Spacer(minLength: 12)
                Text(presentation.current)
                    .font(.system(size: 34 * fontScale, weight: .semibold, design: fontDesign))
                    .lineSpacing(4)
                    .lineLimit(3)
                    .minimumScaleFactor(0.62)
                if !presentation.translation.isEmpty {
                    Text(presentation.translation)
                        .font(.system(size: 19, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .padding(.top, 8)
                }
                Spacer(minLength: 12)
                ProgressView(value: presentation.progress).tint(accent)
            }
        }
    }
}

private struct LocalSummary: View {
    let metrics: SystemMetrics

    var body: some View {
        ContentSurface {
            HStack(spacing: 22) {
                MetricRing(label: "CPU", value: metrics.cpuPercent, color: accent, compact: true)
                MetricRing(label: "内存", value: metrics.memoryPercent, color: .purple, compact: true)
                VStack(alignment: .leading, spacing: 8) {
                    Label(metrics.thermalState, systemImage: "thermometer.medium")
                    Text("↓ \(ByteCountFormatter.string(fromByteCount: Int64(max(0, metrics.networkDownBytesPerSecond)), countStyle: .file))/s")
                    Text("↑ \(ByteCountFormatter.string(fromByteCount: Int64(max(0, metrics.networkUpBytesPerSecond)), countStyle: .file))/s")
                }
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Color.primary.opacity(0.58))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct SourceHealth: View {
    let state: DataSourceState?
    let source: PanelConfiguration.HTTPJSONSource
    var compact = false

    var body: some View {
        let stale = state?.stale == true || state?.lastSuccess.map { Date().timeIntervalSince($0) > source.staleAfterSeconds } == true
        let good = state?.error == nil && !stale && state?.lastSuccess != nil
        HStack(spacing: 7) {
            StatusDot(good: good)
            if !compact {
                Text(stale ? "数据已过期" : state?.error != nil ? "暂时不可用" : good ? "已连接" : "正在加载")
            }
        }
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(Color.primary.opacity(0.58))
    }
}

private struct SourceFieldView: View {
    let field: ResolvedField

    var body: some View {
        ContentSurface {
            VStack(alignment: .leading, spacing: 10) {
                Text(field.label)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Color.primary.opacity(0.48))
                Spacer(minLength: 0)
                HStack(spacing: 10) {
                    if let positive = field.positive { StatusDot(good: positive) }
                    Text(field.text)
                        .font(.system(size: 31, weight: .semibold))
                        .lineLimit(1)
                        .minimumScaleFactor(0.65)
                        .monospacedDigit()
                }
                if field.style == .gauge, let value = field.numericValue {
                    ProgressView(value: min(100, max(0, value)), total: 100).tint(accent)
                }
                Spacer(minLength: 0)
            }
        }
        .frame(minHeight: 150)
    }
}

private struct MetricRing: View {
    let label: String
    let value: Double
    let color: Color
    var compact = false

    var body: some View {
        VStack(spacing: compact ? 5 : 10) {
            ZStack {
                Circle().stroke(Color.primary.opacity(0.08), lineWidth: compact ? 7 : 10)
                Circle()
                    .trim(from: 0, to: min(1, max(0, value / 100)))
                    .stroke(color, style: StrokeStyle(lineWidth: compact ? 7 : 10, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                Text(String(format: "%.0f", value))
                    .font(.system(size: compact ? 20 : 34, weight: .semibold))
                    .monospacedDigit()
            }
            .frame(width: compact ? 70 : 126, height: compact ? 70 : 126)
            Text(label)
                .font(.system(size: compact ? 13 : 16, weight: .semibold))
                .foregroundStyle(Color.primary.opacity(0.54))
        }
    }
}

private struct CompactMetric: View {
    let label: String
    let value: String
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(label).font(.system(size: 14, weight: .semibold)).foregroundStyle(Color.primary.opacity(0.46))
            Text(value).font(.system(size: 25, weight: .semibold)).lineLimit(1).minimumScaleFactor(0.7)
            Text(detail).font(.system(size: 12, weight: .medium)).foregroundStyle(Color.primary.opacity(0.36)).lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct EmptyState: View {
    let symbol: String
    let title: String
    let detail: String

    var body: some View {
        VStack(spacing: 9) {
            Image(systemName: symbol).font(.system(size: 28, weight: .medium)).foregroundStyle(accent)
            Text(title).font(.system(size: 18, weight: .semibold))
            if !detail.isEmpty {
                Text(detail).font(.system(size: 13, weight: .medium)).foregroundStyle(Color.primary.opacity(0.42)).lineLimit(2)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct ContentSurface<Content: View>: View {
    var padding: CGFloat = 18
    @ViewBuilder let content: Content

    init(padding: CGFloat = 18, @ViewBuilder content: () -> Content) {
        self.padding = padding
        self.content = content()
    }

    var body: some View {
        content
            .padding(padding)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .fill(Color(nsColor: .controlBackgroundColor).opacity(0.45))
                    .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).stroke(Color.primary.opacity(0.075), lineWidth: 1))
            )
    }
}

private struct StatusDot: View {
    let good: Bool
    var body: some View {
        Circle().fill(good ? Color.green : Color.orange).frame(width: 9, height: 9)
    }
}

private extension View {
    @ViewBuilder
    func windowGlass() -> some View {
        if #available(macOS 26.0, *) {
            self.glassEffect(.regular.tint(Color(nsColor: .windowBackgroundColor).opacity(0.18)), in: windowShape)
        } else {
            self.background(.ultraThickMaterial, in: windowShape)
        }
    }

    @ViewBuilder
    func controlGlass(cornerRadius: CGFloat) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        if #available(macOS 26.0, *) {
            self.glassEffect(.regular, in: shape)
        } else {
            self.background(.ultraThinMaterial, in: shape).overlay(shape.stroke(Color.primary.opacity(0.10), lineWidth: 1))
        }
    }
}
