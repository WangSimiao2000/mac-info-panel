import SwiftUI

struct LyricsPage: View {
  @ObservedObject var model: DashboardModel
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    TimelineView(
      .periodic(
        from: .now, by: model.lyrics.lines.contains(where: { !$0.tokens.isEmpty }) ? 0.25 : 1.0)
    ) { timeline in
      let presentation = model.lyrics.presentation(at: timeline.date)
      GeometryReader { geometry in
        ContentSurface(padding: 24) {
          HStack(spacing: 28) {
            VStack(spacing: 14) {
              VinylRecordView(playing: model.lyrics.playing)
                .frame(
                  width: min(238, geometry.size.height * 0.58),
                  height: min(238, geometry.size.height * 0.58))
              Text(model.lyrics.title)
                .font(.system(size: 20, weight: .semibold))
                .lineLimit(1)
              Text(model.lyrics.artists)
                .font(.system(size: 15))
                .foregroundStyle(.secondary)
                .lineLimit(1)
            }
            .frame(width: max(220, geometry.size.width * 0.25))

            Divider().overlay(Color.primary.opacity(0.08))

            lyricColumn(presentation)
              .frame(maxWidth: .infinity, maxHeight: .infinity)
          }
        }
      }
    }
  }

  private func lyricColumn(_ presentation: LyricPresentation) -> some View {
    VStack(spacing: 0) {
      HStack {
        Label(
          model.lyrics.playing ? "播放中" : "已暂停",
          systemImage: model.lyrics.playing ? "waveform" : "pause.fill"
        )
        .foregroundStyle(.secondary)
        Spacer()
        Text(PanelFormat.time(presentation.position))
          .font(.system(size: 15, weight: .medium, design: .monospaced))
          .foregroundStyle(.secondary)
      }
      .font(.system(size: 14, weight: .medium))

      Spacer(minLength: 8)
      Text(presentation.previous)
        .font(.system(size: 18))
        .foregroundStyle(Color.primary.opacity(0.42))
        .lineLimit(1)

      Spacer(minLength: 12)
      karaokeText(presentation)
        .font(
          .system(
            size: 44 * lyricFontScale,
            weight: .semibold,
            design: lyricFontDesign
          )
        )
        .multilineTextAlignment(.center)
        .lineSpacing(5)
        .lineLimit(3)
        .minimumScaleFactor(0.62)
        .frame(maxWidth: .infinity)
        .id(presentation.lineID)
        .transition(
          .asymmetric(
            insertion: .move(edge: .bottom).combined(with: .opacity),
            removal: .move(edge: .top).combined(with: .opacity)
          )
        )
        .animation(
          reduceMotion ? nil : .spring(response: 0.48, dampingFraction: 0.82),
          value: presentation.lineID
        )

      if !presentation.translation.isEmpty {
        Text(presentation.translation)
          .font(.system(size: 22 * lyricFontScale, design: lyricFontDesign))
          .foregroundStyle(.secondary)
          .multilineTextAlignment(.center)
          .lineLimit(2)
          .padding(.top, 10)
          .id("translation-\(presentation.lineID ?? -1)")
          .transition(.opacity)
      }

      Spacer(minLength: 12)
      Text(presentation.next)
        .font(.system(size: 20))
        .foregroundStyle(Color.primary.opacity(0.58))
        .lineLimit(1)
      Spacer(minLength: 12)
      ProgressView(value: presentation.progress)
        .tint(PanelTheme.accent)
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
    PanelTheme.lyricFontDesign(model.configuration.appearance)
  }
}
