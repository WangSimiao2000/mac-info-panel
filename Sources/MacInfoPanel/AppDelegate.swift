import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let configuration = PanelConfiguration.load()
    private var model: DashboardModel?
    private var panelController: PanelWindowController?
    private var statusItem: NSStatusItem?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        let model = DashboardModel(configuration: configuration)
        let panelController = PanelWindowController(model: model, screenName: configuration.screenName)
        self.model = model
        self.panelController = panelController

        installStatusMenu()
        panelController.show()
        model.start()
    }

    func applicationWillTerminate(_ notification: Notification) {
        model?.stop()
    }

    private func installStatusMenu() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        item.button?.image = NSImage(systemSymbolName: "rectangle.inset.filled.and.person.filled", accessibilityDescription: "Mac Info Panel")

        let menu = NSMenu()
        let toggle = NSMenuItem(title: "显示/隐藏面板", action: #selector(togglePanel), keyEquivalent: "")
        toggle.target = self
        menu.addItem(toggle)
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "退出", action: #selector(quitApplication), keyEquivalent: "q")
        quit.target = self
        menu.addItem(quit)
        item.menu = menu
        statusItem = item
    }

    @objc private func togglePanel() {
        panelController?.toggle()
    }

    @objc private func quitApplication() {
        NSApp.terminate(nil)
    }
}
