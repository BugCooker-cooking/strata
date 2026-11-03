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
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private var controller: MainWindowController?
    private var controllers: [MainWindowController] = []

    private func makeController() -> MainWindowController {
        let reference = WindowReference()
        let events = EngineEvents { event in reference.controller?.handle(event) }
        let new = MainWindowController(engine: StrataEngine(listener: events))
        reference.controller = new; controllers.append(new)
        new.onNewTab = { [weak self, weak new] in
            guard let self else { return }
            let next = self.makeController()
            if let nextWindow = next.window { new?.window?.addTabbedWindow(nextWindow, ordered: .above) }
            next.showWindow(nil); next.window?.makeKeyAndOrderFront(nil); next.newDocument()
        }
        new.onOpenURL = { [weak self, weak new] url in
            guard let self else { return }
            if let existing = self.controllers.first(where: { $0.documentURL?.resolvingSymlinksInPath() == url.resolvingSymlinksInPath() }) {
                existing.window?.makeKeyAndOrderFront(nil); return
            }
            // Reuse a blank tab; otherwise opening a file creates an independent engine.
            if let new, !new.workspace.hasDocument { new.open(url); return }
            let next = self.makeController()
            if let nextWindow = next.window { new?.window?.addTabbedWindow(nextWindow, ordered: .above) }
            next.showWindow(nil); next.window?.makeKeyAndOrderFront(nil); next.open(url)
        }
        new.onTheme = { [weak self] theme in
            UserDefaults.standard.set(theme.rawValue, forKey: "workspace.theme")
            self?.controllers.forEach { $0.applyTheme(theme) }
        }
        new.onBecameMain = { [weak self, weak new] in
            guard let self, let new else { return }; self.controller = new; self.installMenu(new)
        }
        new.onClosed = { [weak self, weak new] in
            self?.controllers.removeAll { $0 === new }
            if self?.controller === new { self?.controller = self?.controllers.last }
        }
        return new
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Hosted CI may have no GPU. Tests create their own engines/layers as needed.
        guard NSClassFromString("XCTestCase") == nil else { return }
        let controller = makeController()
        self.controller = controller
        installMenu(controller)
        controller.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
        do {
            if let options = try BenchmarkOptions.parse(ProcessInfo.processInfo.arguments) {
                // Opt-in diagnostic mode: ordinary app windows keep their normal level.
                if options.keepVisible { controller.window?.level = .floating }
                let start = CACurrentMediaTime()
                controller.open(options.input, recordRecent: false) { [weak controller] result in
                    do {
                        let info = try result.get()
                        guard let controller else { throw BenchmarkFailure("Window closed during image load") }
                        controller.setBenchmarkRunning(true)
                        try controller.canvas.startBenchmark(options: options, info: info,
                            openMs: (CACurrentMediaTime() - start) * 1000) { result in
                                controller.setBenchmarkRunning(false)
                                switch result {
                                case .success(let url): print("STRATA_BENCHMARK_REPORT \(url.path)")
                                case .failure(let error): print("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
                                }
                                NSApp.terminate(nil)
                            }
                    } catch {
                        controller?.setBenchmarkRunning(false)
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

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let pending = controllers.filter { !$0.canTerminateNow }
        if pending.isEmpty { return .terminateNow }
        confirmTermination(pending, index: 0) { approved in sender.reply(toApplicationShouldTerminate: approved) }
        return .terminateLater
    }
    private func confirmTermination(_ pending: [MainWindowController], index: Int, completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard index < pending.count else { completion(true); return }
        pending[index].window?.makeKeyAndOrderFront(nil)
        pending[index].requestTermination { [weak self] approved in
            if approved { self?.confirmTermination(pending, index: index + 1, completion: completion) } else { completion(false) }
        }
    }

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
        let open = file.addItem(withTitle: "打开文件…", action: #selector(MainWindowController.openImage), keyEquivalent: "o")
        open.target = controller
        let recent = NSMenu(title: "最近打开"); recent.delegate = self
        let recentItem = NSMenuItem(title: "最近打开", action: nil, keyEquivalent: ""); recentItem.submenu = recent; file.addItem(recentItem)
        let place = file.addItem(withTitle: "置入图片…", action: #selector(MainWindowController.placeImage), keyEquivalent: "P"); place.target = controller
        for (title, action, key) in [
            ("新建标签页…", #selector(MainWindowController.newTab), "n"),
            ("保存", #selector(MainWindowController.saveDocument), "s"),
            ("另存为…", #selector(MainWindowController.saveAs), "S"),
            ("导出…", #selector(MainWindowController.exportDocument), "e")
        ] { let item = file.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = controller }
        file.addItem(withTitle: "关闭标签页", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        let fileItem = NSMenuItem(title: "文件", action: nil, keyEquivalent: ""); fileItem.submenu = file; bar.addItem(fileItem)
        let view = NSMenu(title: "视图")
        let edit = NSMenu(title: "编辑")
        for (title, action, key) in [("撤销", #selector(MainWindowController.undo), "z"), ("重做", #selector(MainWindowController.redo), "Z")] {
            let item = edit.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = controller
        }
        edit.addItem(.separator())
        for (title, action, key) in [("剪切", #selector(NSText.cut(_:)), "x"), ("复制", #selector(NSText.copy(_:)), "c"), ("粘贴", #selector(NSText.paste(_:)), "v"), ("全选", #selector(NSText.selectAll(_:)), "a")] {
            edit.addItem(withTitle: title, action: action, keyEquivalent: key)
        }
        let editItem = NSMenuItem(title: "编辑", action: nil, keyEquivalent: ""); editItem.submenu = edit; bar.addItem(editItem)
        for (title, action, key) in [
            ("适合屏幕", #selector(MainWindowController.fit), "0"),
            ("实际像素", #selector(MainWindowController.actualSize), "1"),
            ("显示 / 收起 AI 助手", #selector(MainWindowController.toggleAssistant), "j"),
            ("放大", #selector(MainWindowController.zoomIn), "="),
            ("缩小", #selector(MainWindowController.zoomOut), "-")
        ] {
            let item = view.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = controller
        }
        let viewItem = NSMenuItem(title: "视图", action: nil, keyEquivalent: ""); viewItem.submenu = view; bar.addItem(viewItem)
        let color = NSMenu(title: "色彩")
        for (title, action) in [("转换工作空间为 sRGB", #selector(MainWindowController.convertToSRGB)), ("转换工作空间为 Display P3", #selector(MainWindowController.convertToP3))] {
            let item = color.addItem(withTitle: title, action: action, keyEquivalent: ""); item.target = controller
        }
        let colorItem = NSMenuItem(title: "色彩", action: nil, keyEquivalent: ""); colorItem.submenu = color; bar.addItem(colorItem)
        NSApp.mainMenu = bar
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu.title == "最近打开" else { return }
        menu.removeAllItems()
        for url in RecentFileStore().urls {
            let item = menu.addItem(withTitle: url.lastPathComponent, action: #selector(MainWindowController.openRecent(_:)), keyEquivalent: "")
            item.target = controller; item.representedObject = url; item.toolTip = url.path
        }
        if menu.items.isEmpty { let item = menu.addItem(withTitle: "暂无最近文件", action: nil, keyEquivalent: ""); item.isEnabled = false }
        else { menu.addItem(.separator()); let clear = menu.addItem(withTitle: "清除菜单", action: #selector(clearRecent), keyEquivalent: ""); clear.target = self }
    }
    @objc private func clearRecent() { RecentFileStore().clear() }
}

@MainActor
private final class WindowReference { weak var controller: MainWindowController? }
