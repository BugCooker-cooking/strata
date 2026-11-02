import AppKit

@main
enum StrataApplication {
    @MainActor static func main() {
        let application = NSApplication.shared
        let delegate = AppDelegate()
        application.setActivationPolicy(.regular)
        application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var controller: MainWindowController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let events = EngineEvents { [weak self] event in self?.controller?.handle(event) }
        let engine = StrataEngine(listener: events)
        let controller = MainWindowController(engine: engine)
        self.controller = controller
        installMenu(controller)
        // XCTest creates independent engines; do not auto-open a second image or steal focus.
        guard NSClassFromString("XCTestCase") == nil else { return }
        controller.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
        if let sample = Bundle.main.url(forResource: "sample-photo", withExtension: "jpg") {
            controller.open(sample)
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        controller?.showWindow(nil)
        return true
    }

    private func installMenu(_ controller: MainWindowController) {
        let bar = NSMenu()
        let app = NSMenu()
        app.addItem(withTitle: "关于 Strata", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "退出 Strata", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appItem = NSMenuItem(); appItem.submenu = app; bar.addItem(appItem)
        let file = NSMenu(title: "文件")
        let open = file.addItem(withTitle: "打开图片…", action: #selector(MainWindowController.openImage), keyEquivalent: "o")
        open.target = controller
        let fileItem = NSMenuItem(title: "文件", action: nil, keyEquivalent: ""); fileItem.submenu = file; bar.addItem(fileItem)
        let view = NSMenu(title: "视图")
        for (title, action, key) in [
            ("适合屏幕", #selector(MainWindowController.fit), "0"),
            ("放大", #selector(MainWindowController.zoomIn), "="),
            ("缩小", #selector(MainWindowController.zoomOut), "-")
        ] {
            let item = view.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = controller
        }
        let viewItem = NSMenuItem(title: "视图", action: nil, keyEquivalent: ""); viewItem.submenu = view; bar.addItem(viewItem)
        NSApp.mainMenu = bar
    }
}
