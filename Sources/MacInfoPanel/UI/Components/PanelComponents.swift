import AppKit
import QuartzCore
import SwiftUI

struct ContentSurface<Content: View>: View {
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
          .overlay(
            RoundedRectangle(cornerRadius: 22, style: .continuous)
              .stroke(Color.primary.opacity(0.075), lineWidth: 1)
          )
      )
  }
}

struct StatusDot: View {
  let good: Bool

  var body: some View {
    Circle()
      .fill(good ? Color.green : Color.orange)
      .frame(width: 9, height: 9)
  }
}

struct SourceHealth: View {
  let state: DataSourceState?
  let source: PanelConfiguration.HTTPJSONSource
  var compact = false

  var body: some View {
    let stale =
      state?.stale == true
      || state?.lastSuccess.map { Date().timeIntervalSince($0) > source.staleAfterSeconds } == true
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

struct SourceFieldView: View {
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
          ProgressView(value: min(100, max(0, value)), total: 100)
            .tint(PanelTheme.metricColor(for: value))
        }
        Spacer(minLength: 0)
      }
    }
    .frame(minHeight: 150)
  }
}

struct MetricRing: View {
  let label: String
  let value: Double
  var compact = false

  var body: some View {
    let color = PanelTheme.metricColor(for: value)
    VStack(spacing: compact ? 5 : 10) {
      ZStack {
        Circle().stroke(Color.primary.opacity(0.07), lineWidth: compact ? 6 : 9)
        Circle()
          .trim(from: 0, to: min(1, max(0, value / 100)))
          .stroke(color, style: StrokeStyle(lineWidth: compact ? 6 : 9, lineCap: .round))
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

struct CompactMetric: View {
  let label: String
  let value: String
  let detail: String

  var body: some View {
    VStack(alignment: .leading, spacing: 5) {
      Text(label)
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(Color.primary.opacity(0.46))
      Text(value)
        .font(.system(size: 25, weight: .semibold))
        .lineLimit(1)
        .minimumScaleFactor(0.7)
      Text(detail)
        .font(.system(size: 12, weight: .medium))
        .foregroundStyle(Color.primary.opacity(0.40))
        .lineLimit(1)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

struct EmptyState: View {
  let symbol: String
  let title: String
  let detail: String

  var body: some View {
    VStack(spacing: 9) {
      Image(systemName: symbol)
        .font(.system(size: 28, weight: .medium))
        .foregroundStyle(PanelTheme.accent)
      Text(title).font(.system(size: 18, weight: .semibold))
      if !detail.isEmpty {
        Text(detail)
          .font(.system(size: 13, weight: .medium))
          .foregroundStyle(Color.primary.opacity(0.48))
          .lineLimit(2)
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }
}

struct VinylRecordView: NSViewRepresentable {
  let playing: Bool
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  func makeNSView(context: Context) -> VinylRecordNSView {
    VinylRecordNSView()
  }

  func updateNSView(_ nsView: VinylRecordNSView, context: Context) {
    nsView.isSpinning = playing && !reduceMotion
  }
}

final class VinylRecordNSView: NSView {
  private let shadowLayer = CALayer()
  private let discLayer = VinylDiscLayer()

  var isSpinning = false {
    didSet {
      guard oldValue != isSpinning else { return }
      updateAnimation()
      setAccessibilityLabel(isSpinning ? "唱片正在播放" : "唱片已暂停")
    }
  }

  override init(frame frameRect: NSRect) {
    super.init(frame: frameRect)
    wantsLayer = true
    layer?.addSublayer(shadowLayer)
    layer?.addSublayer(discLayer)
    setAccessibilityElement(true)
    setAccessibilityRole(.image)
  }

  required init?(coder: NSCoder) {
    nil
  }

  override func layout() {
    super.layout()
    let side = max(0, min(bounds.width, bounds.height) - 24)
    let center = CGPoint(x: bounds.midX, y: bounds.midY)
    let discRect = CGRect(x: center.x - side / 2, y: center.y - side / 2, width: side, height: side)

    CATransaction.begin()
    CATransaction.setDisableActions(true)

    shadowLayer.frame = bounds
    shadowLayer.shadowPath = CGPath(ellipseIn: discRect, transform: nil)
    shadowLayer.shadowColor = NSColor.black.cgColor
    shadowLayer.shadowOpacity = 0.28
    shadowLayer.shadowRadius = 14
    shadowLayer.shadowOffset = CGSize(width: 0, height: -6)

    discLayer.anchorPoint = CGPoint(x: 0.5, y: 0.5)
    discLayer.bounds = CGRect(x: 0, y: 0, width: side, height: side)
    discLayer.position = center
    discLayer.cornerRadius = side / 2
    discLayer.contentsScale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
    discLayer.setNeedsDisplay()

    CATransaction.commit()
  }

  private func updateAnimation() {
    discLayer.removeAnimation(forKey: "vinylRotation")
    guard isSpinning else { return }
    let animation = CABasicAnimation(keyPath: "transform.rotation.z")
    animation.fromValue = 0
    animation.toValue = Double.pi * 2
    animation.duration = 12
    animation.repeatCount = .infinity
    animation.timingFunction = CAMediaTimingFunction(name: .linear)
    discLayer.add(animation, forKey: "vinylRotation")
  }
}

private final class VinylDiscLayer: CALayer {
  override init() {
    super.init()
    masksToBounds = true
    needsDisplayOnBoundsChange = true
  }

  override init(layer: Any) {
    super.init(layer: layer)
  }

  required init?(coder: NSCoder) {
    nil
  }

  override func draw(in context: CGContext) {
    let side = min(bounds.width, bounds.height)
    let recordRect = CGRect(
      x: bounds.midX - side / 2,
      y: bounds.midY - side / 2,
      width: side,
      height: side
    )

    context.saveGState()
    context.addEllipse(in: recordRect)
    context.clip()
    let colors =
      [NSColor(calibratedWhite: 0.22, alpha: 1).cgColor, NSColor.black.cgColor] as CFArray
    if let gradient = CGGradient(
      colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1])
    {
      context.drawRadialGradient(
        gradient,
        startCenter: CGPoint(x: recordRect.midX, y: recordRect.midY),
        startRadius: side * 0.08,
        endCenter: CGPoint(x: recordRect.midX, y: recordRect.midY),
        endRadius: side * 0.5,
        options: []
      )
    }
    context.restoreGState()

    context.setLineWidth(1)
    context.setStrokeColor(NSColor.white.withAlphaComponent(0.10).cgColor)
    for scale in [0.30, 0.44, 0.58, 0.72, 0.86] as [CGFloat] {
      let diameter = side * scale
      context.strokeEllipse(
        in: CGRect(
          x: bounds.midX - diameter / 2,
          y: bounds.midY - diameter / 2,
          width: diameter,
          height: diameter
        )
      )
    }

    context.setStrokeColor(NSColor.white.withAlphaComponent(0.24).cgColor)
    context.setLineWidth(3)
    context.setLineCap(.round)
    context.addArc(
      center: CGPoint(x: bounds.midX, y: bounds.midY),
      radius: side * 0.40,
      startAngle: 0.18,
      endAngle: 0.72,
      clockwise: false
    )
    context.strokePath()

    let labelSide = side * 0.30
    context.setFillColor(NSColor.controlAccentColor.cgColor)
    context.fillEllipse(
      in: CGRect(
        x: bounds.midX - labelSide / 2,
        y: bounds.midY - labelSide / 2,
        width: labelSide,
        height: labelSide
      )
    )
    context.setFillColor(NSColor.white.withAlphaComponent(0.55).cgColor)
    context.fillEllipse(
      in: CGRect(
        x: bounds.midX + labelSide * 0.22,
        y: bounds.midY + labelSide * 0.08,
        width: 6,
        height: 6
      )
    )
    context.setFillColor(NSColor.windowBackgroundColor.cgColor)
    context.fillEllipse(in: CGRect(x: bounds.midX - 5, y: bounds.midY - 5, width: 10, height: 10))
  }
}

enum PanelFormat {
  static func duration(_ seconds: Double) -> String {
    let minutes = max(0, Int(seconds)) / 60
    let days = minutes / 1_440
    let hours = (minutes % 1_440) / 60
    if days > 0 { return "\(days)天 \(hours)小时" }
    if hours > 0 { return "\(hours)小时" }
    return "\(minutes)分钟"
  }

  static func time(_ seconds: Double) -> String {
    String(format: "%d:%02d", max(0, Int(seconds)) / 60, max(0, Int(seconds)) % 60)
  }

  static func bytes(_ value: UInt64) -> String {
    ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .file)
  }

  static func rate(_ value: Double) -> String {
    "\(ByteCountFormatter.string(fromByteCount: Int64(max(0, value)), countStyle: .file))/s"
  }
}
