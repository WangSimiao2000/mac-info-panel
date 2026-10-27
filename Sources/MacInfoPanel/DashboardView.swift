import SwiftUI

struct DashboardView: View {
    @ObservedObject var model: DashboardModel

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                background
                .ignoresSafeArea()
                .accessibilityHidden(true)

                VStack(spacing: 14) {
                    header
                    pageContent
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .id(model.selectedPage.id)
                        .transition(.opacity.combined(with: .scale(scale: 0.985)))
                    footer
                }
                .padding(18)
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
        }
        .foregroundStyle(.white)
    }

    private var background: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color(red: 0.025, green: 0.04, blue: 0.075),
                    Color(red: 0.075, green: 0.045, blue: 0.11),
                    Color(red: 0.025, green: 0.08, blue: 0.10),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            Circle()
                .fill(Color.cyan.opacity(0.16))
                .frame(width: 420, height: 420)
                .blur(radius: 90)
                .offset(x: -430, y: -240)
            Circle()
                .fill(Color.purple.opacity(0.18))
                .frame(width: 440, height: 440)
                .blur(radius: 110)
                .offset(x: 430, y: 250)
        }
    }

    private var header: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 2) {
                Text(model.selectedPage.title)
                    .font(.system(size: 34, weight: .bold, design: .rounded))
                Text(model.now, format: .dateTime.month().day().weekday(.wide))
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(.white.opacity(0.62))
            }
            Spacer()
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if model.autoRotateEnabled {
                    Image(systemName: "arrow.triangle.2.circlepath")
                        .font(.system(size: 16, weight: .bold))
                        .foregroundStyle(.cyan)
                }
                Text(model.now, format: .dateTime.hour().minute().second())
                    .font(.system(size: 42, weight: .semibold, design: .rounded))
                    .monospacedDigit()
            }
        }
        .padding(.horizontal, 4)
    }

    @ViewBuilder
    private var pageContent: some View {
        switch model.selectedPage.kind {
        case .overview:
            overviewPage
        case .lyrics:
            lyricsPage
        case .httpJSON:
            sourcePage(sourceID: model.selectedPage.sourceID)
        case .localSystem:
            localSystemPage
        }
    }

    private var overviewPage: some View {
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 14), GridItem(.flexible(), spacing: 14)], spacing: 14) {
            GlassCard(title: "正在播放", symbol: model.lyrics.playing ? "waveform" : "music.note") {
                VStack(alignment: .leading, spacing: 10) {
                    Text(model.lyrics.title)
                        .font(.system(size: 27, weight: .bold, design: .rounded))
                        .lineLimit(1)
                    Text(model.lyrics.artists)
                        .font(.system(size: 17, weight: .medium))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text(model.lyrics.currentLine)
                        .font(.system(size: 24, weight: .semibold, design: .rounded))
                        .lineLimit(2)
                        .minimumScaleFactor(0.72)
                    ProgressView(value: model.lyrics.progress).tint(.orange)
                }
            }

            if let source = model.configuration.sources.first {
                sourceOverview(source)
            } else {
                GlassCard(title: "网络数据源", symbol: "network") {
                    emptyState("在配置文件中添加 HTTPS JSON 数据源", symbol: "plus.circle")
                }
            }

            GlassCard(title: "这台 Mac", symbol: "macmini") {
                VStack(spacing: 12) {
                    gaugeRow("CPU", model.system.cpuPercent)
                    gaugeRow("内存", model.system.memoryPercent)
                    valueRow("热状态", model.system.thermalState)
                    valueRow("运行时间", duration(model.system.uptimeSeconds))
                }
            }

            GlassCard(title: "存储与网络", symbol: "externaldrive.connected.to.line.below") {
                VStack(spacing: 12) {
                    if let volume = model.system.volumes.first {
                        gaugeRow(volume.label, volume.percent)
                        valueRow("可用空间", volume.freeBytes.map(bytes) ?? "未挂载")
                    }
                    valueRow("下载", "↓ \(rate(model.system.networkDownBytesPerSecond))")
                    valueRow("上传", "↑ \(rate(model.system.networkUpBytesPerSecond))")
                }
            }
        }
    }

    private var lyricsPage: some View {
        GlassCard(title: model.lyrics.title, symbol: model.lyrics.playing ? "waveform" : "pause.fill") {
            VStack(alignment: .leading, spacing: 20) {
                Text(model.lyrics.artists)
                    .font(.system(size: 21, weight: .medium))
                    .foregroundStyle(.white.opacity(0.65))
                    .lineLimit(1)
                Spacer(minLength: 0)
                Text(model.lyrics.currentLine)
                    .font(.system(size: 50, weight: .bold, design: .rounded))
                    .lineSpacing(8)
                    .lineLimit(3)
                    .minimumScaleFactor(0.55)
                if !model.lyrics.currentTranslation.isEmpty {
                    Text(model.lyrics.currentTranslation)
                        .font(.system(size: 26, weight: .semibold, design: .rounded))
                        .foregroundStyle(.orange.opacity(0.95))
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
                if !model.lyrics.nextLine.isEmpty {
                    Text(model.lyrics.nextLine)
                        .font(.system(size: 22, weight: .medium))
                        .foregroundStyle(.white.opacity(0.5))
                        .lineLimit(2)
                }
                ProgressView(value: model.lyrics.progress)
                    .tint(.orange)
                    .scaleEffect(y: 1.5)
            }
        }
    }

    @ViewBuilder
    private func sourcePage(sourceID: String?) -> some View {
        if let sourceID, let source = model.configuration.sources.first(where: { $0.id == sourceID }) {
            let state = model.sourceStates[sourceID]
            VStack(spacing: 14) {
                HStack {
                    Label(source.title, systemImage: source.symbol)
                        .font(.system(size: 27, weight: .bold, design: .rounded))
                    Spacer()
                    sourceStatus(state, source: source)
                }
                .padding(.horizontal, 6)

                if let fields = state?.snapshot?.fields, !fields.isEmpty {
                    LazyVGrid(columns: [GridItem(.flexible(), spacing: 14), GridItem(.flexible(), spacing: 14)], spacing: 14) {
                        ForEach(fields) { field in
                            FieldTile(field: field)
                        }
                    }
                } else {
                    GlassCard(title: "等待数据", symbol: "network.slash") {
                        emptyState(state?.error ?? "正在连接 \(source.url.host ?? source.url.absoluteString)", symbol: "arrow.clockwise")
                    }
                }
            }
        } else {
            GlassCard(title: "配置错误", symbol: "exclamationmark.triangle") {
                emptyState("页面没有对应的数据源", symbol: "wrench.and.screwdriver")
            }
        }
    }

    private var localSystemPage: some View {
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 14), GridItem(.flexible(), spacing: 14), GridItem(.flexible(), spacing: 14)], spacing: 14) {
            MetricTile(label: "CPU", value: String(format: "%.0f%%", model.system.cpuPercent), symbol: "cpu", gauge: model.system.cpuPercent)
            MetricTile(label: "内存", value: String(format: "%.0f%%", model.system.memoryPercent), symbol: "memorychip", gauge: model.system.memoryPercent)
            MetricTile(label: "系统负载", value: String(format: "%.2f", model.system.loadAverage.0), symbol: "chart.xyaxis.line", detail: String(format: "5 分钟 %.2f · 15 分钟 %.2f", model.system.loadAverage.1, model.system.loadAverage.2))
            MetricTile(label: "热状态", value: model.system.thermalState, symbol: "thermometer.medium", detail: "无需管理员权限")
            ForEach(model.system.volumes) { volume in
                MetricTile(label: volume.label, value: volume.freeBytes.map(bytes) ?? "未挂载", symbol: "externaldrive", gauge: volume.percent, detail: volume.path)
            }
            MetricTile(label: "网络", value: "↓ \(rate(model.system.networkDownBytesPerSecond))", symbol: "network", detail: "↑ \(rate(model.system.networkUpBytesPerSecond))")
            MetricTile(label: "运行时间", value: duration(model.system.uptimeSeconds), symbol: "clock.arrow.circlepath")
        }
    }

    private func sourceOverview(_ source: PanelConfiguration.HTTPJSONSource) -> some View {
        let state = model.sourceStates[source.id]
        return GlassCard(title: source.title, symbol: source.symbol) {
            if let fields = state?.snapshot?.fields {
                VStack(spacing: 11) {
                    ForEach(Array(fields.prefix(4))) { field in
                        valueRow(field.label, field.text, positive: field.positive)
                    }
                    Spacer(minLength: 0)
                    sourceStatus(state, source: source)
                }
            } else {
                emptyState(state?.error ?? "正在加载", symbol: "arrow.clockwise")
            }
        }
    }

    private func sourceStatus(_ state: DataSourceState?, source: PanelConfiguration.HTTPJSONSource) -> some View {
        let staleByAge = state?.lastSuccess.map { Date().timeIntervalSince($0) > source.staleAfterSeconds } ?? false
        let warning = state?.stale == true || staleByAge
        let text: String
        if warning { text = "数据已过期" }
        else if state?.error != nil { text = "暂时不可用" }
        else if let date = state?.lastSuccess { text = "更新于 \(date.formatted(date: .omitted, time: .shortened))" }
        else { text = "正在加载" }
        return HStack(spacing: 7) {
            StatusDot(good: state?.error == nil && !warning)
            Text(text)
        }
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(.white.opacity(0.65))
    }

    private var footer: some View {
        HStack {
            if let warning = model.configurationWarning {
                Label(warning, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .lineLimit(1)
            } else {
                Label("本地优先 · 只读 GET · 无管理员权限", systemImage: "lock.shield")
                    .foregroundStyle(.white.opacity(0.55))
            }
            Spacer()
            HStack(spacing: 8) {
                ForEach(Array(model.configuration.pages.enumerated()), id: \.element.id) { index, page in
                    Capsule()
                        .fill(index == model.selectedPageIndex ? Color.white : Color.white.opacity(0.25))
                        .frame(width: index == model.selectedPageIndex ? 24 : 8, height: 8)
                        .accessibilityLabel(page.title)
                }
            }
        }
        .font(.system(size: 13, weight: .medium))
        .padding(.horizontal, 6)
    }

    private func valueRow(_ label: String, _ value: String, positive: Bool? = nil) -> some View {
        HStack(spacing: 12) {
            Text(label).foregroundStyle(.white.opacity(0.55))
            Spacer()
            if let positive { StatusDot(good: positive) }
            Text(value).fontWeight(.bold).monospacedDigit().lineLimit(1)
        }
        .font(.system(size: 17))
    }

    private func gaugeRow(_ label: String, _ value: Double?) -> some View {
        VStack(spacing: 7) {
            valueRow(label, value.map { String(format: "%.0f%%", $0) } ?? "—")
            ProgressView(value: min(100, max(0, value ?? 0)), total: 100)
                .tint(metricColor(value ?? 0))
        }
    }

    private func emptyState(_ text: String, symbol: String) -> some View {
        VStack(spacing: 13) {
            Image(systemName: symbol).font(.system(size: 38)).foregroundStyle(.cyan)
            Text(text)
                .font(.system(size: 18, weight: .semibold))
                .multilineTextAlignment(.center)
                .foregroundStyle(.white.opacity(0.65))
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func metricColor(_ value: Double) -> Color {
        if value >= 90 { return .red }
        if value >= 70 { return .orange }
        return .cyan
    }

    private func duration(_ seconds: Double) -> String {
        let minutes = max(0, Int(seconds)) / 60
        let days = minutes / 1_440
        let hours = (minutes % 1_440) / 60
        let remainder = minutes % 60
        if days > 0 { return "\(days)天 \(hours)小时" }
        if hours > 0 { return "\(hours)小时 \(remainder)分" }
        return "\(remainder)分钟"
    }

    private func bytes(_ value: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .file)
    }

    private func rate(_ value: Double) -> String {
        "\(ByteCountFormatter.string(fromByteCount: Int64(max(0, value)), countStyle: .file))/s"
    }
}

private struct GlassCard<Content: View>: View {
    let title: String
    let symbol: String
    @ViewBuilder let content: Content

    init(title: String, symbol: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.symbol = symbol
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label(title, systemImage: symbol)
                .font(.system(size: 17, weight: .bold))
                .foregroundStyle(.white.opacity(0.72))
                .lineLimit(1)
            content
        }
        .padding(19)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .panelGlass(cornerRadius: 24)
    }
}

private struct FieldTile: View {
    let field: ResolvedField

    var body: some View {
        GlassCard(title: field.label, symbol: field.style == .status ? "circle.fill" : "waveform.path.ecg") {
            VStack(alignment: .leading, spacing: 12) {
                Spacer(minLength: 0)
                HStack(spacing: 12) {
                    if let positive = field.positive { StatusDot(good: positive) }
                    Text(field.text)
                        .font(.system(size: 34, weight: .bold, design: .rounded))
                        .minimumScaleFactor(0.6)
                        .lineLimit(2)
                }
                if field.style == .gauge, let value = field.numericValue {
                    ProgressView(value: min(100, max(0, value)), total: 100).tint(.cyan)
                }
                Spacer(minLength: 0)
            }
        }
        .frame(minHeight: 112)
    }
}

private struct MetricTile: View {
    let label: String
    let value: String
    let symbol: String
    var gauge: Double?
    var detail: String?

    var body: some View {
        GlassCard(title: label, symbol: symbol) {
            VStack(alignment: .leading, spacing: 12) {
                Spacer(minLength: 0)
                Text(value)
                    .font(.system(size: 33, weight: .bold, design: .rounded))
                    .minimumScaleFactor(0.65)
                    .lineLimit(1)
                    .monospacedDigit()
                if let gauge {
                    ProgressView(value: min(100, max(0, gauge)), total: 100).tint(.cyan)
                }
                if let detail {
                    Text(detail).font(.system(size: 14, weight: .medium)).foregroundStyle(.white.opacity(0.48)).lineLimit(1)
                }
                Spacer(minLength: 0)
            }
        }
        .frame(minHeight: 112)
    }
}

private struct StatusDot: View {
    let good: Bool
    var body: some View {
        Circle().fill(good ? Color.green : Color.orange).frame(width: 11, height: 11)
    }
}

private extension View {
    @ViewBuilder
    func panelGlass(cornerRadius: CGFloat) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        if #available(macOS 26.0, *) {
            self.glassEffect(.regular.tint(.white.opacity(0.04)), in: shape)
        } else {
            self
                .background(.ultraThinMaterial, in: shape)
                .overlay(shape.stroke(.white.opacity(0.11), lineWidth: 1))
        }
    }
}
