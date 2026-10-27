import AppKit
import CoreGraphics
import SwiftUI

private final class PageSwitchingHostingView<Content: View>: NSHostingView<Content> {
    var onPreviousPage: (() -> Void)?
    var onNextPage: (() -> Void)?

    private var accumulatedDelta: CGFloat = 0
    private var gestureTriggered = false
    private var lastMouseWheelSwitch = Date.distantPast

    override func scrollWheel(with event: NSEvent) {
        let delta = abs(event.scrollingDeltaX) > abs(event.scrollingDeltaY)
            ? event.scrollingDeltaX
            : event.scrollingDeltaY

        if event.phase == .began {
            accumulatedDelta = 0
            gestureTriggered = false
        }
        if event.momentumPhase != [] && gestureTriggered { return }

        if event.hasPreciseScrollingDeltas {
            accumulatedDelta += delta
            guard !gestureTriggered, abs(accumulatedDelta) >= 28 else { return }
            gestureTriggered = true
            switchPage(delta: accumulatedDelta)
        } else {
            guard abs(delta) > 0, Date().timeIntervalSince(lastMouseWheelSwitch) >= 0.35 else { return }
            lastMouseWheelSwitch = Date()
            switchPage(delta: delta)
        }

        if event.phase == .ended || event.phase == .cancelled {
            accumulatedDelta = 0
            gestureTriggered = false
        }
    }

    private func switchPage(delta: CGFloat) {
        if delta > 0 { onPreviousPage?() }
        else { onNextPage?() }
    }
}

private final class PassivePanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect {
        frameRect
    }
}

@MainActor
final class PanelWindowController {
    private let display: PanelConfiguration.Display
    private let panel: PassivePanel
    private var screenObserver: NSObjectProtocol?

    init(model: DashboardModel, display: PanelConfiguration.Display) {
        self.display = display
        panel = PassivePanel(
            contentRect: .zero,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        let hostingView = PageSwitchingHostingView(rootView: DashboardView(model: model))
        hostingView.onPreviousPage = { [weak model] in model?.previousPage() }
        hostingView.onNextPage = { [weak model] in model?.nextPage() }
        hostingView.wantsLayer = true
        hostingView.layer?.cornerRadius = 30
        hostingView.layer?.cornerCurve = .continuous
        hostingView.layer?.masksToBounds = true
        panel.contentView = hostingView
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.invalidateShadow()
        panel.level = .normal
        panel.hidesOnDeactivate = false
        panel.ignoresMouseEvents = false
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]
        panel.animationBehavior = .none

        screenObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.placeOnTargetScreen() }
        }
    }

    func show() {
        placeOnTargetScreen()
    }

    func toggle() {
        if panel.isVisible { panel.orderOut(nil) }
        else { placeOnTargetScreen() }
    }

    private func placeOnTargetScreen() {
        guard let screen = targetScreen() else {
            panel.orderOut(nil)
            return
        }
        let padding = max(0, display.padding)
        var frame = screen.visibleFrame.insetBy(dx: padding, dy: padding)
        let visibleDoesNotReserveMenuBar = abs(screen.visibleFrame.maxY - screen.frame.maxY) < 1
            && abs(screen.visibleFrame.height - screen.frame.height) < 1
        if visibleDoesNotReserveMenuBar {
            frame.size.height = max(100, frame.height - max(30, NSStatusBar.system.thickness))
        }
        panel.setFrame(frame, display: true)
        panel.orderFrontRegardless()
    }

    private func targetScreen() -> NSScreen? {
        if let name = display.nameContains, !name.isEmpty,
           let screen = NSScreen.screens.first(where: { $0.localizedName.localizedCaseInsensitiveContains(name) }) {
            return screen
        }
        if let vendorID = display.vendorID, let modelID = display.modelID,
           let screen = NSScreen.screens.first(where: { hardwareMatches($0, vendorID: vendorID, modelID: modelID) }) {
            return screen
        }
        return NSScreen.screens.first(where: { $0 != NSScreen.main }) ?? NSScreen.main
    }

    private func hardwareMatches(_ screen: NSScreen, vendorID: UInt32, modelID: UInt32) -> Bool {
        let key = NSDeviceDescriptionKey("NSScreenNumber")
        guard let number = screen.deviceDescription[key] as? NSNumber else { return false }
        let displayID = CGDirectDisplayID(number.uint32Value)
        return CGDisplayVendorNumber(displayID) == vendorID && CGDisplayModelNumber(displayID) == modelID
    }
}
