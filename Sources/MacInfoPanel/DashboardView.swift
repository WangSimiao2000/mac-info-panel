import SwiftUI

struct DashboardView: View {
    @ObservedObject var model: DashboardModel

    var body: some View {
        ZStack {
            Color(red: 0.035, green: 0.045, blue: 0.065)
            HStack(spacing: 18) {
                lyricsColumn
                    .frame(width: 510)
                minecraftColumn
                    .frame(width: 355)
                systemColumn
                    .frame(maxWidth: .infinity)
            }
            .padding(24)
        }
        .frame(minWidth: 960, minHeight: 540)
        .foregroundStyle(.white)
    }

    private var lyricsColumn: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .firstTextBaseline) {
                Text(model.now, format: .dateTime.hour().minute().second())
                    .font(.system(size: 50, weight: .semibold, design: .rounded))
                    .monospacedDigit()
                Spacer()
                Text(model.now, format: .dateTime.month().day().weekday(.wide))
                    .font(.system(size: 18, weight: .medium))
                    .foregroundStyle(.secondary)
            }

            PanelCard(title: "正在播放", symbol: model.lyrics.playing ? "waveform" : "pause.fill") {
                VStack(alignment: .leading, spacing: 10) {
                    Text(model.lyrics.title)
                        .font(.system(size: 25, weight: .semibold))
                        .lineLimit(1)
                    Text(model.lyrics.artists)
                        .font(.system(size: 17))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    ProgressView(value: model.lyrics.progress)
                        .tint(.orange)
                }
            }

            PanelCard(title: "歌词", symbol: "quote.bubble") {
                VStack(alignment: .leading, spacing: 16) {
                    Spacer(minLength: 4)
                    Text(model.lyrics.currentLine)
                        .font(.system(size: 31, weight: .bold, design: .rounded))
                        .lineLimit(3)
                        .minimumScaleFactor(0.72)
                    if !model.lyrics.currentTranslation.isEmpty {
                        Text(model.lyrics.currentTranslation)
                            .font(.system(size: 19, weight: .medium))
                            .foregroundStyle(Color.orange.opacity(0.9))
                            .lineLimit(2)
                    }
                    if !model.lyrics.nextLine.isEmpty {
                        Divider().overlay(.white.opacity(0.12))
                        Text(model.lyrics.nextLine)
                            .font(.system(size: 18))
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }
                    Spacer(minLength: 4)
                }
                .frame(maxHeight: .infinity)
            }
            .frame(maxHeight: .infinity)

            StatusLine(
                text: model.lyrics.error ?? "OmniLyrics 本地连接正常",
                good: model.lyrics.error == nil
            )
        }
    }

    private var minecraftColumn: some View {
        VStack(alignment: .leading, spacing: 18) {
            PanelCard(title: "Minecraft", symbol: "server.rack") {
                if let snapshot = model.minecraft {
                    VStack(alignment: .leading, spacing: 18) {
                        HStack {
                            StatusDot(good: snapshot.minecraft?.online == true || snapshot.minecraft?.serviceActive == true)
                            Text(mcState(snapshot))
                                .font(.system(size: 27, weight: .bold))
                            Spacer()
                        }
                        metricRow("在线玩家", value: players(snapshot))
                        metricRow("游戏版本", value: snapshot.minecraft?.version ?? "—")
                        metricRow("服务时长", value: duration(snapshot.minecraft?.uptimeSeconds))
                        Divider().overlay(.white.opacity(0.12))
                        metricRow("Agent", value: snapshot.agent?.health ?? "未知")
                        metricRow("Agent 版本", value: snapshot.agent?.version ?? "—")
                    }
                } else {
                    VStack(spacing: 14) {
                        Image(systemName: "key.slash")
                            .font(.system(size: 42))
                            .foregroundStyle(.orange)
                        Text("等待只读 SSH Key")
                            .font(.title3.weight(.semibold))
                        Text(model.minecraftError ?? "服务器连接尚未配置")
                            .multilineTextAlignment(.center)
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(height: 330)

            PanelCard(title: "服务器主机", symbol: "cpu") {
                VStack(spacing: 14) {
                    metricBar("CPU", value: model.minecraft?.host?.cpuPercent)
                    metricBar("内存", value: model.minecraft?.host?.memoryPercent)
                    metricBar("磁盘", value: model.minecraft?.host?.diskPercent)
                    metricRow("负载", value: decimal(model.minecraft?.host?.load1))
                    metricRow("运行时间", value: duration(model.minecraft?.host?.uptimeSeconds))
                }
            }
            .frame(maxHeight: .infinity)

            StatusLine(
                text: model.minecraftError ?? "mc-ops 只读连接正常",
                good: model.minecraftError == nil
            )
        }
    }

    private var systemColumn: some View {
        VStack(alignment: .leading, spacing: 18) {
            PanelCard(title: "这台 Mac", symbol: "macmini") {
                VStack(spacing: 15) {
                    metricBar("CPU", value: model.system.cpuPercent)
                    metricBar("内存", value: model.system.memoryPercent)
                    metricRow("内存", value: "\(bytes(model.system.memoryUsedBytes)) / \(bytes(model.system.memoryTotalBytes))")
                    metricRow("系统负载", value: String(format: "%.2f  %.2f  %.2f", model.system.loadAverage.0, model.system.loadAverage.1, model.system.loadAverage.2))
                    metricRow("热状态", value: model.system.thermalState)
                    metricRow("运行时间", value: duration(model.system.uptimeSeconds))
                }
            }
            .frame(height: 330)

            PanelCard(title: "存储与网络", symbol: "externaldrive") {
                VStack(spacing: 15) {
                    metricBar("内置盘", value: model.system.rootDiskPercent)
                    metricRow("内置盘可用", value: bytes(model.system.rootDiskFreeBytes))
                    metricBar("exSSD", value: model.system.externalDiskPercent)
                    metricRow("exSSD 可用", value: model.system.externalDiskFreeBytes.map(bytes) ?? "未挂载")
                    Divider().overlay(.white.opacity(0.12))
                    metricRow("下载", value: "↓ \(rate(model.system.networkDownBytesPerSecond))")
                    metricRow("上传", value: "↑ \(rate(model.system.networkUpBytesPerSecond))")
                }
            }
            .frame(maxHeight: .infinity)

            StatusLine(text: "低权限模式 · 不采集真实瓦数", good: true)
        }
    }

    private func metricRow(_ name: String, value: String) -> some View {
        HStack {
            Text(name).foregroundStyle(.secondary)
            Spacer()
            Text(value).fontWeight(.semibold).monospacedDigit()
        }
        .font(.system(size: 16))
    }

    private func metricBar(_ name: String, value: Double?) -> some View {
        VStack(spacing: 6) {
            metricRow(name, value: value.map { String(format: "%.0f%%", $0) } ?? "—")
            ProgressView(value: min(100, max(0, value ?? 0)), total: 100)
                .tint(color(for: value ?? 0))
        }
    }

    private func color(for value: Double) -> Color {
        if value >= 90 { return .red }
        if value >= 70 { return .orange }
        return .cyan
    }

    private func mcState(_ snapshot: MCPanelSnapshot) -> String {
        if snapshot.minecraft?.online == true { return "运行中" }
        return snapshot.minecraft?.state ?? (snapshot.minecraft?.serviceActive == true ? "服务已启动" : "离线")
    }

    private func players(_ snapshot: MCPanelSnapshot) -> String {
        guard let online = snapshot.minecraft?.playersOnline else { return "—" }
        return snapshot.minecraft?.playersMax.map { "\(online) / \($0)" } ?? "\(online)"
    }

    private func decimal(_ value: Double?) -> String {
        value.map { String(format: "%.2f", $0) } ?? "—"
    }

    private func duration(_ seconds: Double?) -> String {
        guard let seconds, seconds >= 0 else { return "—" }
        let totalMinutes = Int(seconds) / 60
        let days = totalMinutes / 1440
        let hours = (totalMinutes % 1440) / 60
        let minutes = totalMinutes % 60
        if days > 0 { return "\(days)天 \(hours)小时" }
        if hours > 0 { return "\(hours)小时 \(minutes)分" }
        return "\(minutes)分钟"
    }

    private func bytes(_ value: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .file)
    }

    private func rate(_ value: Double) -> String {
        "\(ByteCountFormatter.string(fromByteCount: Int64(max(0, value)), countStyle: .file))/s"
    }
}

private struct PanelCard<Content: View>: View {
    let title: String
    let symbol: String
    @ViewBuilder let content: Content

    init(title: String, symbol: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.symbol = symbol
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label(title, systemImage: symbol)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(.white.opacity(0.82))
            content
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.white.opacity(0.065), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).stroke(Color.white.opacity(0.08)))
    }
}

private struct StatusDot: View {
    let good: Bool
    var body: some View {
        Circle().fill(good ? Color.green : Color.orange).frame(width: 11, height: 11)
    }
}

private struct StatusLine: View {
    let text: String
    let good: Bool

    var body: some View {
        HStack(spacing: 8) {
            StatusDot(good: good)
            Text(text).lineLimit(1)
        }
        .font(.system(size: 13, weight: .medium))
        .foregroundStyle(.secondary)
    }
}
