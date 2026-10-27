import SwiftUI

struct HTTPJSONPage: View {
  @ObservedObject var model: DashboardModel
  let sourceID: String?

  var body: some View {
    if let sourceID, let source = model.configuration.sources.first(where: { $0.id == sourceID }) {
      let state = model.sourceStates[sourceID]
      VStack(spacing: 12) {
        HStack {
          Label(source.title, systemImage: source.symbol)
            .font(.system(size: 25, weight: .semibold))
          Spacer()
          SourceHealth(state: state, source: source)
        }
        .padding(.horizontal, 5)

        if let fields = state?.snapshot?.fields, !fields.isEmpty {
          LazyVGrid(
            columns: Array(repeating: GridItem(.flexible(), spacing: 12), count: 3),
            spacing: 12
          ) {
            ForEach(fields) { field in SourceFieldView(field: field) }
          }
          .frame(maxHeight: .infinity)
        } else {
          ContentSurface {
            EmptyState(
              symbol: "network.slash",
              title: "等待数据",
              detail: state?.error ?? source.url.host ?? source.url.absoluteString
            )
          }
        }
      }
    } else {
      ContentSurface {
        EmptyState(symbol: "exclamationmark.triangle", title: "配置错误", detail: "页面没有对应的数据源")
      }
    }
  }
}
