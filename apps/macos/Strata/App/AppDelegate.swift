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
    private var recoveryDirectory: URL?
    private var memoryPressure: DispatchSourceMemoryPressure?

    // DispatchSource invokes this on its utility queue. Creating the callback
    // outside MainActor prevents Swift 6 from asserting main-queue isolation.
    // A global callback must not retain the first tab's document after close.
    nonisolated static func makeMemoryPressureHandler() -> @Sendable () -> Void {
        { @Sendable in try? trimGlobalSourceMemory() }
    }

    private func makeController() -> MainWindowController {
        let reference = WindowReference()
        let events = EngineEvents { event in reference.controller?.handle(event) }
        let new = MainWindowController(engine: StrataEngine(listener: events))
        reference.controller = new; controllers.append(new)
        new.syncRecentFiles()
        if let recoveryDirectory { new.startRecovery(directory: recoveryDirectory) }
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
        do {
            recoveryDirectory = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("Strata/Recovery", isDirectory: true)
        } catch { NSLog("Recovery directory unavailable: %@", error.localizedDescription) }
        let controller = makeController()
        self.controller = controller
        installMenu(controller)
        controller.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
        let pressure = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .global(qos: .utility))
        pressure.setEventHandler(handler: Self.makeMemoryPressureHandler()); pressure.resume(); memoryPressure = pressure
        do {
            if let options = try WorkspacePressureCheck.Options.parse(ProcessInfo.processInfo.arguments) {
                var checks = [controller]
                for _ in options.inputs.dropFirst() {
                    let next = makeController()
                    if let window = next.window { controller.window?.addTabbedWindow(window, ordered: .above) }
                    checks.append(next)
                }
                Task {
                    do { try await WorkspacePressureCheck.run(options, controllers: checks) }
                    catch { benchmarkLog("STRATA_WORKSPACE_PRESSURE_FAILED \(error.localizedDescription)") }
                    // Leave the MainActor task before AppKit enters its nested
                    // termination loop, so recovery cleanup can reenter main.
                    RunLoop.main.perform(inModes: [.common]) {
                        MainActor.assumeIsolated { NSApp.terminate(nil) }
                    }
                }
                return
            }
            if let options = try BenchmarkOptions.parse(ProcessInfo.processInfo.arguments) {
                // Opt-in diagnostic mode: ordinary app windows keep their normal level.
                if options.keepVisible {
                    controller.window?.level = .floating
                    controller.window?.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
                }
                let start = CACurrentMediaTime()
                benchmarkLog("STRATA_BENCHMARK_OPEN_BEGIN visible=\(controller.window?.isVisible ?? false) minimized=\(controller.window?.isMiniaturized ?? false)")
                controller.open(options.input, recordRecent: false) { [weak controller] result in
                    do {
                        let info = try result.get()
                        guard let controller else { throw BenchmarkFailure("Window closed during image load") }
                        benchmarkLog("STRATA_BENCHMARK_OPEN_DONE ms=\((CACurrentMediaTime() - start) * 1000) depth=\(info.bitDepth)")
                        // File preparation is asynchronous and can outlast a
                        // window activation/restoration transition. A view
                        // display link will not run while its window is miniaturized.
                        controller.window?.deminiaturize(nil)
                        controller.window?.makeKeyAndOrderFront(nil)
                        NSApp.activate(ignoringOtherApps: true)
                        controller.window?.contentView?.layoutSubtreeIfNeeded()
                        controller.setBenchmarkRunning(true)
                        try controller.canvas.startBenchmark(options: options, info: info,
                            openMs: (CACurrentMediaTime() - start) * 1000) { result in
                                controller.setBenchmarkRunning(false)
                                switch result {
                                case .success(let url): benchmarkLog("STRATA_BENCHMARK_REPORT \(url.path)")
                                case .failure(let error): benchmarkLog("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
                                }
                                NSApp.terminate(nil)
                            }
                    } catch {
                        controller?.setBenchmarkRunning(false)
                        benchmarkLog("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
                        NSApp.terminate(nil)
                    }
                }
                return
            }
        } catch {
            benchmarkLog("STRATA_BENCHMARK_FAILED \(error.localizedDescription)")
            NSApp.terminate(nil)
            return
        }
        if presentRecovery() { return }
        if let sample = Bundle.main.url(forResource: "sample-photo", withExtension: "jpg") {
            controller.open(sample)
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        memoryPressure?.cancel()
    }
    @objc private func showRecovery() { _ = presentRecovery() }
    @discardableResult private func presentRecovery() -> Bool {
        guard let controller, !controller.busy, let window = controller.window, window.attachedSheet == nil else { return false }
        do {
            let active = Set(controllers.compactMap { $0.state?.sessionId } + controllers.compactMap { $0.recovery?.recoveredEntry })
            let entries = try RecoveryEntry.decode(controller.engine.recoveryEntriesJson()).filter { !active.contains($0.id) }
            guard !entries.isEmpty else { return false }
            let alert = NSAlert(); alert.messageText = "找到 \(entries.count) 份未保存的恢复副本"
            let names = entries.prefix(5).map { $0.info?.label ?? "无法读取的副本" }.joined(separator: "\n")
            alert.informativeText = "\(names)\n\n恢复后将作为未保存文档打开，请另存工程。无法读取的副本会保留，供再次尝试。"
            alert.addButton(withTitle: "恢复副本"); alert.addButton(withTitle: "稍后"); alert.addButton(withTitle: "丢弃这些副本")
            alert.beginSheetModal(for: window) { [weak self] response in
                guard let self else { return }
                if response == .alertFirstButtonReturn { self.restore(entries, index: 0) }
                else if response == .alertThirdButtonReturn {
                    for entry in entries { do { try controller.engine.discardRecovery(id: entry.id) } catch { controller.showError(error); break } }
                }
            }
            return true
        } catch { controller.showError(error); return false }
    }
    private func restore(_ entries: [RecoveryEntry], index: Int) {
        guard index < entries.count else { return }
        let entry = entries[index]
        guard let info = entry.info else { restore(entries, index: index + 1); return }
        let next: MainWindowController
        if let controller, !controller.workspace.hasDocument { next = controller }
        else {
            next = makeController()
            if let window = next.window { controller?.window?.addTabbedWindow(window, ordered: .above) }
        }
        next.showWindow(nil); next.window?.makeKeyAndOrderFront(nil)
        do {
            let resource = try next.engine.grantRead(path: entry.path)
            next.perform("file.recover", ["resource": resource]) { [weak self, weak next] ok in
                if ok, let next { next.documentURL = nil; next.fileName = info.label + "（恢复）"; next.recovery?.recoveredEntry = entry.id; next.refresh() }
                self?.restore(entries, index: index + 1)
            }
        } catch { next.showError(error) }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let pending = controllers.filter { !$0.canTerminateNow }
        if pending.isEmpty { finishTermination(sender); return .terminateLater }
        confirmTermination(pending, index: 0) { [weak self] approved in
            if approved { self?.finishTermination(sender) } else { sender.reply(toApplicationShouldTerminate: false) }
        }
        return .terminateLater
    }
    private func finishTermination(_ sender: NSApplication) {
        let group = DispatchGroup()
        for controller in controllers {
            controller.busy = true
            if let monitor = controller.recovery { group.enter(); monitor.stop(discard: true) { group.leave() } }
        }
        group.notify(queue: .main) { sender.reply(toApplicationShouldTerminate: true) }
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
        controller.syncRecentFiles()
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
        let recovery = file.addItem(withTitle: "恢复未保存文档…", action: #selector(showRecovery), keyEquivalent: ""); recovery.target = self
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
        let stopAI = edit.addItem(withTitle: "停止当前操作", action: #selector(MainWindowController.stopAI), keyEquivalent: "."); stopAI.target = controller
        edit.addItem(.separator())
        for (title, action, key) in [("剪切", #selector(MainWindowController.cutPixels(_:)), "x"), ("复制", #selector(MainWindowController.copyPixels(_:)), "c"), ("粘贴", #selector(MainWindowController.pastePixels(_:)), "v"), ("全选", #selector(MainWindowController.selectAllPixels(_:)), "a"),("合并拷贝",#selector(MainWindowController.copyMergedPixels(_:)),"C"),("原位粘贴",#selector(MainWindowController.pasteInPlace(_:)),"V"),("清除",#selector(MainWindowController.clearPixels(_:)),"")] {
            let item=edit.addItem(withTitle: title, action: action, keyEquivalent: key);item.target=controller
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
        for (title, action) in [("指定 sRGB（保留数值）",#selector(MainWindowController.assignSRGB)),("指定 Display P3（保留数值）",#selector(MainWindowController.assignP3)),("转换为 sRGB（保持颜色）", #selector(MainWindowController.convertToSRGB)), ("转换为 Display P3（保持颜色）", #selector(MainWindowController.convertToP3))] {
            let item = color.addItem(withTitle: title, action: action, keyEquivalent: ""); item.target = controller
        }
        let colorItem = NSMenuItem(title: "色彩", action: nil, keyEquivalent: ""); colorItem.submenu = color; bar.addItem(colorItem)
        NSApp.mainMenu = bar
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu.title == "最近打开" else { return }
        menu.removeAllItems()
        controller?.syncRecentFiles()
        let json = try? controller?.engine.dispatch(command: "file.recent.list", paramsJson: "{}")
        let values = json.flatMap { try? JSONDecoder().decode(HostRecentList.self, from: Data($0.utf8)) }?.files ?? []
        for value in values {
            guard let path = value.path else { continue }
            let url = URL(fileURLWithPath: path)
            let item = menu.addItem(withTitle: value.name, action: #selector(MainWindowController.openRecent(_:)), keyEquivalent: "")
            item.target = controller; item.representedObject = url; item.toolTip = value.available ? path : "文件不可用：\(path)"; item.isEnabled = value.available
        }
        if menu.items.isEmpty { let item = menu.addItem(withTitle: "暂无最近文件", action: nil, keyEquivalent: ""); item.isEnabled = false }
        else { menu.addItem(.separator()); let clear = menu.addItem(withTitle: "清除菜单", action: #selector(clearRecent), keyEquivalent: ""); clear.target = self }
    }
    @objc private func clearRecent() { RecentFileStore().clear(); controllers.forEach { $0.syncRecentFiles() } }
}

private struct HostRecentList: Decodable {
    struct Entry: Decodable { let name: String; let path: String?; let available: Bool }
    let files: [Entry]
}

@MainActor
private final class WindowReference { weak var controller: MainWindowController? }
