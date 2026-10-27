import SwiftUI

struct DashboardView: View {
  @ObservedObject var model: DashboardModel
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    VStack(spacing: 12) {
      topBar
      ZStack {
        pageContent
          .id(model.selectedPage.id)
          .transition(pageTransition)
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .animation(pageAnimation, value: model.selectedPageIndex)
      pageIndicator
    }
    .padding(14)
    .foregroundStyle(.primary)
    .systemWindowSurface()
    .clipShape(PanelTheme.windowShape)
    .overlay(PanelTheme.windowShape.stroke(Color.primary.opacity(0.12), lineWidth: 1))
    .shadow(color: .black.opacity(0.28), radius: 20, y: 8)
    .padding(2)
  }

  private var topBar: some View {
    HStack(spacing: 12) {
      Image(systemName: pageSymbol)
        .font(.system(size: 17, weight: .semibold))
        .foregroundStyle(PanelTheme.accent)
      Text(model.selectedPage.title)
        .font(.system(size: 19, weight: .semibold))
      if let warning = model.configurationWarning {
        Image(systemName: "exclamationmark.triangle.fill")
          .foregroundStyle(.orange)
          .help(warning)
      }
      Spacer()
      Text(model.now, format: .dateTime.month().day().weekday(.abbreviated))
        .font(.system(size: 15))
        .foregroundStyle(.secondary)
      Text(model.now, format: .dateTime.hour().minute())
        .font(.system(size: 25, weight: .semibold))
        .monospacedDigit()
    }
    .padding(.horizontal, 6)
    .frame(height: 48)
  }

  @ViewBuilder
  private var pageContent: some View {
    switch model.selectedPage.kind {
    case .overview:
      OverviewPage(model: model)
    case .lyrics:
      LyricsPage(model: model)
    case .httpJSON:
      HTTPJSONPage(model: model, sourceID: model.selectedPage.sourceID)
    case .localSystem:
      LocalSystemPage(model: model)
    }
  }

  private var pageIndicator: some View {
    HStack(spacing: 8) {
      if model.autoRotateEnabled {
        Image(systemName: "arrow.triangle.2.circlepath")
          .font(.system(size: 11, weight: .semibold))
          .foregroundStyle(PanelTheme.accent)
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

  private var pageTransition: AnyTransition {
    guard !reduceMotion else { return .identity }
    return .asymmetric(
      insertion: .offset(x: model.pageTransitionDirection > 0 ? 110 : -110, y: 0)
        .combined(with: .opacity)
        .combined(with: .scale(scale: 0.97)),
      removal: .offset(x: model.pageTransitionDirection > 0 ? -90 : 90, y: 0)
        .combined(with: .opacity)
        .combined(with: .scale(scale: 0.985))
    )
  }

  private var pageAnimation: Animation? {
    reduceMotion ? nil : .spring(response: 0.52, dampingFraction: 0.82)
  }

  private var pageSymbol: String {
    switch model.selectedPage.kind {
    case .overview: "rectangle.grid.2x2"
    case .lyrics: "quote.bubble"
    case .httpJSON: "server.rack"
    case .localSystem: "macmini"
    }
  }
}
