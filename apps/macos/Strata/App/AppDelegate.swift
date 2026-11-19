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
    private var exportPresetRestored=false
    private var appPreferencesRestored = false

    // DispatchSource invokes this on its utility queue. Creating the callback
    // outside MainActor prevents Swift 6 from asserting main-queue isolation.
    // A global callback must not retain the first tab's document after close.
    nonisolated static func makeMemoryPressureHandler() -> @Sendable () -> Void {
        { @Sendable in try? trimGlobalSourceMemory() }
    }

    private func makeController() -> MainWindowController {
        let reference = WindowReference()
        let events = EngineEvents { event in reference.controller?.handle(event) }
        return registerController(StrataEngine(listener: events), reference: reference, name: L10n.text("未命名"))
    }

    private func registerController(_ engine: StrataEngine, reference: WindowReference, name: String) -> MainWindowController {
        if !appPreferencesRestored {
            appPreferencesRestored = true
            try? ApplicationPreferences.shared.restore(on: engine)
        }
        let new = MainWindowController(engine: engine, fileName: name)
        reference.controller = new; controllers.append(new)
        if !exportPresetRestored{exportPresetRestored=true;do{try QuickExportPreferences().restore(on:new.engine)}catch{new.quickExportError=L10n.format("无法读取快速导出预设，请重新设置。{0}", String(describing: error.localizedDescription))}}
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
        new.onDocumentDuplicated = { [weak self, weak new] name, activate, copy in
            guard let self, let new else { return }
            self.showDuplicate(name: name, activate: activate, copy: copy, source: new)
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

    private func showDuplicate(name: String, activate: Bool, copy: DuplicatedDocument, source: MainWindowController) {
        guard let parent = source.window, controllers.contains(where: { $0 === source }) else { return }
        do {
            let reference = WindowReference()
            let engine = try copy.takeEngine(listener: EngineEvents { event in reference.controller?.handle(event) })
            let selected = parent.tabGroup?.selectedWindow ?? parent
            let next = registerController(engine, reference: reference, name: name)
            guard let window = next.window else { return }
            parent.addTabbedWindow(window, ordered: .above)
            if activate {
                next.showWindow(nil)
                window.makeKeyAndOrderFront(nil)
            } else {
                window.tabGroup?.selectedWindow = selected
            }
            next.canvas.fit()
        } catch {
            source.showError(error)
        }
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
            let alert = NSAlert(); alert.messageText = L10n.format("找到 {0} 份未保存的恢复副本", String(describing: entries.count))
            let names = entries.prefix(5).map { $0.info?.label ?? L10n.text("无法读取的副本") }.joined(separator: "\n")
            alert.informativeText = L10n.format("{0}\n\n恢复后将作为未保存文档打开，请另存工程。无法读取的副本会保留，供再次尝试。", String(describing: names))
            alert.addButton(withTitle: L10n.text("恢复副本")); alert.addButton(withTitle: L10n.text("稍后")); alert.addButton(withTitle: L10n.text("丢弃这些副本"))
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
                if ok, let next { next.documentURL = nil; next.fileName = info.label + L10n.text("（恢复）"); next.recovery?.recoveredEntry = entry.id; next.refresh() }
                self?.restore(entries, index: index + 1)
            }
        } catch { next.showError(error) }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        // Real sheet fixtures must not terminate the XCTest host between cases.
        NSClassFromString("XCTestCase") == nil
    }

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

    @objc private func setInterfaceLanguage(_ sender: NSMenuItem) {
        guard let code = sender.representedObject as? String, let language = InterfaceLanguage(rawValue: code),
              language != L10n.preferredLanguage() else { return }
        L10n.setPreferredLanguage(language)
        sender.menu?.items.forEach { $0.state = $0 === sender ? .on : .off }
        guard language != L10n.language, let window = controller?.window, window.attachedSheet == nil else { return }
        let alert = NSAlert()
        alert.messageText = L10n.text("已保存界面语言")
        alert.informativeText = L10n.text("再次打开 Strata 时使用所选语言。")
        alert.addButton(withTitle: L10n.text("好"))
        alert.beginSheetModal(for: window)
    }

    private func installMenu(_ controller: MainWindowController) {
        controller.syncRecentFiles()
        let bar = NSMenu()
        let app = NSMenu()
        app.addItem(withTitle: L10n.text("关于 Strata"), action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        let preferences = app.addItem(withTitle: L10n.text("偏好设置…"), action: #selector(MainWindowController.showPreferences), keyEquivalent: ",")
        preferences.target = controller
        let title = L10n.text("界面语言")
        let languages = NSMenu(title: title)
        for language in InterfaceLanguage.allCases {
            let item = languages.addItem(withTitle: language.name, action: #selector(setInterfaceLanguage(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = language.rawValue
            item.state = language == L10n.preferredLanguage() ? .on : .off
        }
        let languageItem = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        languageItem.submenu = languages
        app.addItem(languageItem)
        app.addItem(.separator())
        app.addItem(withTitle: L10n.text("退出 Strata"), action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appItem = NSMenuItem(); appItem.submenu = app; bar.addItem(appItem)
        let file = NSMenu(title: L10n.text("文件"))
        let open = file.addItem(withTitle: L10n.text("打开文件…"), action: #selector(MainWindowController.openImage), keyEquivalent: "o")
        open.target = controller
        let recent = NSMenu(title: L10n.text("最近打开")); recent.delegate = self
        let recentItem = NSMenuItem(title: L10n.text("最近打开"), action: nil, keyEquivalent: ""); recentItem.submenu = recent; file.addItem(recentItem)
        let recovery = file.addItem(withTitle: L10n.text("恢复未保存文档…"), action: #selector(showRecovery), keyEquivalent: ""); recovery.target = self
        let place = file.addItem(withTitle: L10n.text("置入图片…"), action: #selector(MainWindowController.placeImage), keyEquivalent: "P"); place.target = controller
        for (title, action, key) in [
            (L10n.text("新建标签页…"), #selector(MainWindowController.newTab), "n"),
            (L10n.text("保存"), #selector(MainWindowController.saveDocument), "s"),
            (L10n.text("另存为…"), #selector(MainWindowController.saveAs), "S"),
            (L10n.text("恢复到上次保存…"),#selector(MainWindowController.revertDocument),""),
            (L10n.text("导出…"), #selector(MainWindowController.exportDocument), "e"),
            (L10n.text("导出分层 PSD…"), #selector(MainWindowController.exportLayeredPSD), ""),
            (L10n.text("快速导出…"), #selector(MainWindowController.quickExportDocument), "E"),
            (L10n.text("快速导出预设…"), #selector(MainWindowController.configureQuickExport), "")
        ] { let item = file.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = controller }
        file.addItem(withTitle: L10n.text("关闭标签页"), action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        let fileItem = NSMenuItem(title: L10n.text("文件"), action: nil, keyEquivalent: ""); fileItem.submenu = file; bar.addItem(fileItem)
        let view = NSMenu(title: L10n.text("视图"))
        let edit = NSMenu(title: L10n.text("编辑"))
        for (title, action, key) in [(L10n.text("撤销"), #selector(MainWindowController.undo), "z"), (L10n.text("重做"), #selector(MainWindowController.redo), "Z")] {
            let item = edit.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = controller
        }
        edit.addItem(.separator())
        let stopAI = edit.addItem(withTitle: L10n.text("停止当前操作"), action: #selector(MainWindowController.stopAI), keyEquivalent: "."); stopAI.target = controller
        edit.addItem(.separator())
        for (title, action, key) in [(L10n.text("剪切"), #selector(MainWindowController.cutPixels(_:)), "x"), (L10n.text("复制"), #selector(MainWindowController.copyPixels(_:)), "c"), (L10n.text("粘贴"), #selector(MainWindowController.pastePixels(_:)), "v"), (L10n.text("全选"), #selector(MainWindowController.selectAllPixels(_:)), "a"),(L10n.text("合并拷贝"),#selector(MainWindowController.copyMergedPixels(_:)),"C"),(L10n.text("原位粘贴"),#selector(MainWindowController.pasteInPlace(_:)),"V"),(L10n.text("清除"),#selector(MainWindowController.clearPixels(_:)),"")] {
            let item=edit.addItem(withTitle: title, action: action, keyEquivalent: key);item.target=controller
        }
        edit.addItem(.separator())
        let transform=edit.addItem(withTitle:L10n.text("自由变换"),action:#selector(MainWindowController.beginCanvasTransform),keyEquivalent:"t");transform.target=controller
        edit.addItem(.separator())
        let search = edit.addItem(withTitle: L10n.text("功能搜索…"), action: #selector(MainWindowController.showCommandSearch), keyEquivalent: "f")
        search.target = controller
        let editItem = NSMenuItem(title: L10n.text("编辑"), action: nil, keyEquivalent: ""); editItem.submenu = edit; bar.addItem(editItem)
        for (title, action, key) in [
            (L10n.text("适合屏幕"), #selector(MainWindowController.fit), "0"),
            (L10n.text("实际像素"), #selector(MainWindowController.actualSize), "1"),
            (L10n.text("显示 / 收起 AI 助手"), #selector(MainWindowController.toggleAssistant), "j"),
            (L10n.text("放大"), #selector(MainWindowController.zoomIn), "="),
            (L10n.text("缩小"), #selector(MainWindowController.zoomOut), "-")
        ] {
            let item = view.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = controller
        }
        view.addItem(.separator())
        for(title,action,key)in[(L10n.text("标尺"),#selector(MainWindowController.toggleRulers(_:)),"r"),(L10n.text("参考线"),#selector(MainWindowController.toggleGuides(_:)),";"),(L10n.text("锁定参考线"),#selector(MainWindowController.toggleGuideLock(_:)),""),(L10n.text("吸附"),#selector(MainWindowController.toggleSnap(_:)),""),(L10n.text("参考线与吸附设置"),#selector(MainWindowController.showGuides),"")]{let item=view.addItem(withTitle:title,action:action,keyEquivalent:key);item.target=controller}
        let viewItem = NSMenuItem(title: L10n.text("视图"), action: nil, keyEquivalent: ""); viewItem.submenu = view; bar.addItem(viewItem)
        let image=NSMenu(title:L10n.text("图像"))
        let duplicate = image.addItem(withTitle: L10n.text("复制文档…"), action: #selector(MainWindowController.duplicateDocument), keyEquivalent: "")
        duplicate.target = controller
        image.addItem(.separator())
        for(title,action)in[(L10n.text("自动色调"),#selector(MainWindowController.autoTone)),(L10n.text("自动对比度"),#selector(MainWindowController.autoContrast)),(L10n.text("自动颜色"),#selector(MainWindowController.autoColor))]{let item=image.addItem(withTitle:title,action:action,keyEquivalent:"");item.target=controller}
        image.addItem(.separator())
        for(title,action) in [(L10n.text("转换为 8 位 / 通道"),#selector(MainWindowController.convertToEightBit)),(L10n.text("转换为 16 位 / 通道"),#selector(MainWindowController.convertToSixteenBit))]{let item=image.addItem(withTitle:title,action:action,keyEquivalent:"");item.target=controller}
        let imageItem=NSMenuItem(title:L10n.text("图像"),action:nil,keyEquivalent:"");imageItem.submenu=image;bar.addItem(imageItem)
        let color = NSMenu(title: L10n.text("色彩"))
        for (title, action) in [(L10n.text("指定 sRGB（保留数值）"),#selector(MainWindowController.assignSRGB)),(L10n.text("指定 Display P3（保留数值）"),#selector(MainWindowController.assignP3)),(L10n.text("转换为 sRGB（保持颜色）"), #selector(MainWindowController.convertToSRGB)), (L10n.text("转换为 Display P3（保持颜色）"), #selector(MainWindowController.convertToP3))] {
            let item = color.addItem(withTitle: title, action: action, keyEquivalent: ""); item.target = controller
        }
        let colorItem = NSMenuItem(title: L10n.text("色彩"), action: nil, keyEquivalent: ""); colorItem.submenu = color; bar.addItem(colorItem)
        NSApp.mainMenu = bar
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu.title == L10n.text("最近打开") else { return }
        menu.removeAllItems()
        controller?.syncRecentFiles()
        let json = try? controller?.engine.dispatch(command: "file.recent.list", paramsJson: "{}")
        let values = json.flatMap { try? JSONDecoder().decode(HostRecentList.self, from: Data($0.utf8)) }?.files ?? []
        for value in values {
            guard let path = value.path else { continue }
            let url = URL(fileURLWithPath: path)
            let item = menu.addItem(withTitle: value.name, action: #selector(MainWindowController.openRecent(_:)), keyEquivalent: "")
            item.target = controller; item.representedObject = url; item.toolTip = value.available ? path : L10n.format("文件不可用：{0}", String(describing: path)); item.isEnabled = value.available
        }
        if menu.items.isEmpty { let item = menu.addItem(withTitle: L10n.text("暂无最近文件"), action: nil, keyEquivalent: ""); item.isEnabled = false }
        else { menu.addItem(.separator()); let clear = menu.addItem(withTitle: L10n.text("清除菜单"), action: #selector(clearRecent), keyEquivalent: ""); clear.target = self }
    }
    @objc private func clearRecent() { RecentFileStore().clear(); controllers.forEach { $0.syncRecentFiles() } }
}

private struct HostRecentList: Decodable {
    struct Entry: Decodable { let name: String; let path: String?; let available: Bool }
    let files: [Entry]
}

@MainActor
private final class WindowReference { weak var controller: MainWindowController? }
