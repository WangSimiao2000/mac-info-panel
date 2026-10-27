import AppKit
import CoreGraphics
import SwiftUI

private final class PassivePanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class PanelWindowController {
    private let screenName: String
    private let panel: PassivePanel
    private var screenObserver: NSObjectProtocol?

    init(model: DashboardModel, screenName: String) {
        self.screenName = screenName
        panel = PassivePanel(
            contentRect: .zero,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.contentView = NSHostingView(rootView: DashboardView(model: model))
        panel.backgroundColor = .black
        panel.isOpaque = true
        panel.hasShadow = false
        panel.level = .normal
        panel.hidesOnDeactivate = false
        panel.ignoresMouseEvents = true
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
        if panel.isVisible {
            panel.orderOut(nil)
        } else {
            placeOnTargetScreen()
        }
    }

    private func placeOnTargetScreen() {
        guard let screen = targetScreen() else {
            panel.orderOut(nil)
            return
        }
        panel.setFrame(screen.frame, display: true)
        panel.orderFrontRegardless()
    }

    private func targetScreen() -> NSScreen? {
        if let named = NSScreen.screens.first(where: { $0.localizedName.localizedCaseInsensitiveContains(screenName) }) {
            return named
        }
        if let hardwareMatch = NSScreen.screens.first(where: isWokyis) {
            return hardwareMatch
        }
        return NSScreen.screens.first(where: { $0 != NSScreen.main })
    }

    private func isWokyis(_ screen: NSScreen) -> Bool {
        let key = NSDeviceDescriptionKey("NSScreenNumber")
        guard let number = screen.deviceDescription[key] as? NSNumber else { return false }
        let displayID = CGDirectDisplayID(number.uint32Value)
        return CGDisplayVendorNumber(displayID) == 0x1253 && CGDisplayModelNumber(displayID) == 0x2555
    }
}
