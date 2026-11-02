import AppKit
import QuartzCore

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
        // Hosted CI may have no GPU. Tests create their own engines/layers as needed.
        guard NSClassFromString("XCTestCase") == nil else { return }
        let events = EngineEvents { [weak self] event in self?.controller?.handle(event) }
        let engine = StrataEngine(listener: events)
        let controller = MainWindowController(engine: engine)
        self.controller = controller
        installMenu(controller)
        controller.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
        do {
            if let options = try BenchmarkOptions.parse(ProcessInfo.processInfo.arguments) {
                // Opt-in diagnostic mode: ordinary app windows keep their normal level.
                if options.keepVisible { controller.window?.level = .floating }
                let start = CACurrentMediaTime()
                controller.open(options.input) { [weak controller] result in
                    do {
                        let info = try result.get()
                        guard let controller else { throw BenchmarkFailure("Window closed during image load") }
                        try controller.canvas.startBenchmark(options: options, info: info,
                            openMs: (CACurrentMediaTime() - start) * 1000) { result in
                                switch result {
                                case .success(let url): print("STRATA_BENCHMARK_REPORT \(url.path)")
                                case .failure(let error): print("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
                                }
                                NSApp.terminate(nil)
                            }
                    } catch {
                        print("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
                        NSApp.terminate(nil)
                    }
                }
                return
            }
        } catch {
            print("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
            NSApp.terminate(nil)
            return
        }
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
