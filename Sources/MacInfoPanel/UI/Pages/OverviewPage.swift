import SwiftUI

struct OverviewPage: View {
  @ObservedObject var model: DashboardModel

  var body: some View {
    GeometryReader { geometry in
      HStack(spacing: 12) {
        TimelineView(.periodic(from: .now, by: 1.0)) { timeline in
          NowPlayingOverview(
            snapshot: model.lyrics,
            date: timeline.date,
            fontDesign: PanelTheme.lyricFontDesign(model.configuration.appearance),
            fontScale: model.configuration.appearance?.lyricFontScale ?? 1
          )
        }
        .frame(width: geometry.size.width * 0.59)

        VStack(spacing: 12) {
          if let source = model.configuration.sources.first {
            SourceSummary(source: source, state: model.sourceStates[source.id])
          } else {
            ContentSurface {
              EmptyState(
                symbol: "network", title: "没有数据源", detail: "在 config.jsonc 中添加 HTTPS JSON 接口")
            }
          }
          LocalSummary(metrics: model.system)
        }
      }
    }
  }
}

private struct NowPlayingOverview: View {
  let snapshot: LyricSnapshot
  let date: Date
  let fontDesign: Font.Design
  let fontScale: Double
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    let presentation = snapshot.presentation(at: date)
    ContentSurface(padding: 24) {
      VStack(alignment: .leading, spacing: 0) {
        HStack(spacing: 9) {
          Image(systemName: snapshot.playing ? "waveform" : "music.note")
            .foregroundStyle(PanelTheme.accent)
          Text("正在播放")
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(Color.primary.opacity(0.55))
          Spacer()
          Text(snapshot.playing ? "播放中" : "已暂停")
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(Color.primary.opacity(0.46))
        }
        Text(snapshot.title)
          .font(.system(size: 28, weight: .semibold))
          .lineLimit(1)
          .padding(.top, 16)
        Text(snapshot.artists)
          .font(.system(size: 17))
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .padding(.top, 4)
        Spacer(minLength: 12)
        Text(presentation.current)
          .font(.system(size: 34 * fontScale, weight: .semibold, design: fontDesign))
          .lineSpacing(4)
          .lineLimit(3)
          .minimumScaleFactor(0.62)
          .id(presentation.lineID)
          .transition(.opacity.combined(with: .move(edge: .bottom)))
          .animation(
            reduceMotion ? nil : .spring(response: 0.46, dampingFraction: 0.84),
            value: presentation.lineID
          )
        if !presentation.translation.isEmpty {
          Text(presentation.translation)
            .font(.system(size: 19 * fontScale, design: fontDesign))
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .padding(.top, 8)
        }
        Spacer(minLength: 12)
        ProgressView(value: presentation.progress).tint(PanelTheme.accent)
      }
    }
  }
}

private struct SourceSummary: View {
  let source: PanelConfiguration.HTTPJSONSource
  let state: DataSourceState?

  var body: some View {
    ContentSurface {
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
              Text(field.label).foregroundStyle(.secondary)
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
}

private struct LocalSummary: View {
  let metrics: SystemMetrics

  var body: some View {
    ContentSurface {
      HStack(spacing: 22) {
        MetricRing(label: "CPU", value: metrics.cpuPercent, compact: true)
        MetricRing(label: "内存", value: metrics.memoryPercent, compact: true)
        VStack(alignment: .leading, spacing: 8) {
          Label(metrics.thermalState, systemImage: "thermometer.medium")
          Text("↓ \(PanelFormat.rate(metrics.networkDownBytesPerSecond))")
          Text("↑ \(PanelFormat.rate(metrics.networkUpBytesPerSecond))")
        }
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(.secondary)
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }
}
