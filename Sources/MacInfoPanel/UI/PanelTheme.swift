import AppKit
import SwiftUI

enum PanelTheme {
  static let accent = Color.accentColor
  static let windowShape = RoundedRectangle(cornerRadius: 30, style: .continuous)

  static func lyricFontDesign(_ appearance: PanelConfiguration.Appearance?) -> Font.Design {
    switch appearance?.lyricFontDesign ?? .system {
    case .system: return .default
    case .rounded: return .rounded
    case .serif: return .serif
    case .monospaced: return .monospaced
    }
  }

  static func metricColor(for value: Double) -> Color {
    if value >= 90 { return .red.opacity(0.78) }
    if value >= 75 { return .orange.opacity(0.72) }
    return Color(nsColor: .secondaryLabelColor).opacity(0.62)
  }
}

extension View {
  func systemWindowSurface() -> some View {
    self.background(Color(nsColor: .windowBackgroundColor), in: PanelTheme.windowShape)
  }

  @ViewBuilder
  func controlGlass(cornerRadius: CGFloat) -> some View {
    let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
    if #available(macOS 26.0, *) {
      self.glassEffect(.regular, in: shape)
    } else {
      self
        .background(.ultraThinMaterial, in: shape)
        .overlay(shape.stroke(Color.primary.opacity(0.10), lineWidth: 1))
    }
  }
}
