import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var model: DashboardModel?
    private var panelController: PanelWindowController?
    private var statusItem: NSStatusItem?
    private var autoRotateItem: NSMenuItem?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        let loaded = PanelConfiguration.load()
        let model = DashboardModel(configuration: loaded.configuration, configurationWarning: loaded.warning)
        let panelController = PanelWindowController(model: model, display: loaded.configuration.display)
        self.model = model
        self.panelController = panelController

        installStatusMenu(configuration: loaded.configuration)
        autoLaunchLyricsIfNeeded(loaded.configuration.lyrics)
        panelController.show()
        model.start()
    }

    func applicationWillTerminate(_ notification: Notification) {
        model?.stop()
    }

    private func installStatusMenu(configuration: PanelConfiguration) {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        item.button?.image = NSImage(systemSymbolName: "rectangle.3.group", accessibilityDescription: "Mac Info Panel")

        let menu = NSMenu()
        let toggle = NSMenuItem(title: "显示/隐藏面板", action: #selector(togglePanel), keyEquivalent: "")
        toggle.target = self
        menu.addItem(toggle)
        menu.addItem(.separator())

        for (index, page) in configuration.pages.enumerated() {
            let pageItem = NSMenuItem(title: page.title, action: #selector(selectPage(_:)), keyEquivalent: "")
            pageItem.target = self
            pageItem.tag = index
            menu.addItem(pageItem)
        }

        menu.addItem(.separator())
        let previous = NSMenuItem(title: "上一页", action: #selector(previousPage), keyEquivalent: "[")
        previous.target = self
        menu.addItem(previous)
        let next = NSMenuItem(title: "下一页", action: #selector(nextPage), keyEquivalent: "]")
        next.target = self
        menu.addItem(next)

        let auto = NSMenuItem(title: "自动轮播", action: #selector(toggleAutoRotate), keyEquivalent: "")
        auto.target = self
        auto.state = model?.autoRotateEnabled == true ? .on : .off
        menu.addItem(auto)
        autoRotateItem = auto

        menu.addItem(.separator())
        let config = NSMenuItem(title: "打开配置文件夹", action: #selector(openConfigurationFolder), keyEquivalent: "")
        config.target = self
        menu.addItem(config)
        let quit = NSMenuItem(title: "退出", action: #selector(quitApplication), keyEquivalent: "q")
        quit.target = self
        menu.addItem(quit)

        item.menu = menu
        statusItem = item
    }

    private func autoLaunchLyricsIfNeeded(_ lyrics: PanelConfiguration.Lyrics) {
        guard lyrics.enabled, lyrics.autoLaunch,
              let applicationURL = NSWorkspace.shared.urlForApplication(withBundleIdentifier: lyrics.bundleIdentifier)
        else { return }
        let options = NSWorkspace.OpenConfiguration()
        options.activates = false
        options.addsToRecentItems = false
        options.hides = true
        NSWorkspace.shared.openApplication(at: applicationURL, configuration: options)
    }

    @objc private func togglePanel() { panelController?.toggle() }
    @objc private func previousPage() { model?.previousPage() }
    @objc private func nextPage() { model?.nextPage() }

    @objc private func selectPage(_ sender: NSMenuItem) {
        model?.selectPage(at: sender.tag)
    }

    @objc private func toggleAutoRotate() {
        model?.toggleAutoRotate()
        autoRotateItem?.state = model?.autoRotateEnabled == true ? .on : .off
    }

    @objc private func openConfigurationFolder() {
        let path = NSString(string: "~/.config/mac-info-panel").expandingTildeInPath
        try? FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
        NSWorkspace.shared.open(URL(fileURLWithPath: path))
    }

    @objc private func quitApplication() { NSApp.terminate(nil) }
}
