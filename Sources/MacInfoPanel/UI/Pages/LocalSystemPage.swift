import SwiftUI

struct LocalSystemPage: View {
  @ObservedObject var model: DashboardModel

  var body: some View {
    GeometryReader { geometry in
      HStack(spacing: 12) {
        ContentSurface {
          HStack(spacing: 28) {
            MetricRing(label: "CPU", value: model.system.cpuPercent)
            MetricRing(label: "内存", value: model.system.memoryPercent)
          }
          .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: geometry.size.width * 0.43)

        VStack(spacing: 12) {
          ContentSurface {
            HStack(spacing: 24) {
              CompactMetric(
                label: "系统负载",
                value: String(format: "%.2f", model.system.loadAverage.0),
                detail: "5m  \(String(format: "%.2f", model.system.loadAverage.1))"
              )
              Divider().overlay(Color.primary.opacity(0.08))
              CompactMetric(label: "热状态", value: model.system.thermalState, detail: "无需管理员权限")
              Divider().overlay(Color.primary.opacity(0.08))
              CompactMetric(
                label: "运行时间",
                value: PanelFormat.duration(model.system.uptimeSeconds),
                detail: "macOS"
              )
            }
          }
          ContentSurface {
            HStack(spacing: 18) {
              ForEach(model.system.volumes.prefix(2)) { volume in
                CompactMetric(
                  label: volume.label,
                  value: volume.freeBytes.map(PanelFormat.bytes) ?? "未挂载",
                  detail: "可用空间"
                )
              }
              Spacer(minLength: 0)
              CompactMetric(
                label: "网络下载",
                value: PanelFormat.rate(model.system.networkDownBytesPerSecond),
                detail: "上传 \(PanelFormat.rate(model.system.networkUpBytesPerSecond))"
              )
            }
          }
        }
      }
    }
  }
}
