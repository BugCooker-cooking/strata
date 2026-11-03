import AppKit
import UniformTypeIdentifiers

@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSTableViewDataSource, NSTableViewDelegate {
    let engine: StrataEngine
    let canvas: CanvasView
    private let status = NSTextField(labelWithString: "打开图片或新建文档")
    private let table = NSTableView()
    private let nameField = NSTextField(string: "")
    private let opacity = NSSlider(value: 100, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let visibility = NSButton(checkboxWithTitle: "显示图层", target: nil, action: nil)
    private let blend = NSPopUpButton()
    private let history = NSTextView()
    private var state: EditorSnapshot?
    private var busy = false
    private var refreshing = false
    private var allowClose = false
    private var controls: [NSControl] = []
    private var fileName = "未命名"
    var hasUnsavedChanges: Bool { state?.dirty == true }
    var canTerminateNow: Bool { !busy && (allowClose || !hasUnsavedChanges) }

    func setBenchmarkRunning(_ running: Bool) {
        busy = running
        controls.forEach { $0.isEnabled = !running }
        table.isEnabled = !running
        canvas.isEditingEnabled = !running
    }

    init(engine: StrataEngine) {
        self.engine = engine; canvas = CanvasView(engine: engine)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init(window: window)
        window.title = "Strata"; window.subtitle = "M1a · 图层与安全保存"
        window.minSize = NSSize(width: 1040, height: 640); window.toolbarStyle = .unified
        window.isReleasedWhenClosed = false; window.delegate = self
        let content = NSView(); window.contentView = content
        let header = NSStackView(); header.orientation = .horizontal; header.spacing = 8
        for (title, action) in [("新建", #selector(newDocument)), ("打开…", #selector(openImage)), ("保存", #selector(saveDocument)), ("导出…", #selector(exportDocument)), ("撤销", #selector(undo)), ("重做", #selector(redo)), ("适合屏幕", #selector(fit))] {
            let button = NSButton(title: title, target: self, action: action); header.addArrangedSubview(button); controls.append(button)
        }
        let move = NSButton(checkboxWithTitle: "拖动图层", target: self, action: #selector(toggleMove(_:)))
        move.toolTip = "开启后拖动当前图层，Esc 取消；关闭时拖动画布。滚动和捏合始终调整视图。"
        header.addArrangedSubview(move); controls.append(move)
        status.textColor = .secondaryLabelColor
        let sidebar = NSStackView(); sidebar.orientation = .vertical; sidebar.alignment = .leading; sidebar.spacing = 10
        let title = NSTextField(labelWithString: "图层"); title.font = .systemFont(ofSize: 14, weight: .semibold); sidebar.addArrangedSubview(title)
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("layer")); column.title = "图层"; table.addTableColumn(column)
        table.headerView = nil; table.rowHeight = 30; table.delegate = self; table.dataSource = self; table.allowsEmptySelection = true
        table.setAccessibilityLabel("图层列表")
        let scroll = NSScrollView(); scroll.documentView = table; scroll.hasVerticalScroller = true; sidebar.addArrangedSubview(scroll)
        scroll.widthAnchor.constraint(equalToConstant: 244).isActive = true; scroll.heightAnchor.constraint(equalToConstant: 190).isActive = true
        let buttons = NSStackView(); buttons.spacing = 6
        for (title, action) in [("新增", #selector(createLayer)), ("复制", #selector(duplicateLayer)), ("删除", #selector(deleteLayer)), ("上移", #selector(raiseLayer)), ("下移", #selector(lowerLayer))] {
            let b = NSButton(title: title, target: self, action: action); buttons.addArrangedSubview(b); controls.append(b)
        }
        sidebar.addArrangedSubview(buttons)
        nameField.placeholderString = "图层名称"; nameField.target = self; nameField.action = #selector(renameLayer); sidebar.addArrangedSubview(nameField)
        nameField.widthAnchor.constraint(equalToConstant: 244).isActive = true
        visibility.target = self; visibility.action = #selector(setVisibility); sidebar.addArrangedSubview(visibility)
        sidebar.addArrangedSubview(NSTextField(labelWithString: "不透明度"))
        opacity.target = self; opacity.action = #selector(setOpacity); opacity.isContinuous = false; opacity.setAccessibilityLabel("图层不透明度")
        sidebar.addArrangedSubview(opacity); opacity.widthAnchor.constraint(equalToConstant: 244).isActive = true
        blend.addItems(withTitles: ["正常", "正片叠底", "滤色", "叠加"]); blend.target = self; blend.action = #selector(setBlend); sidebar.addArrangedSubview(blend)
        controls += [nameField, visibility, opacity, blend]
        let historyTitle = NSTextField(labelWithString: "历史与文件记录"); historyTitle.font = .systemFont(ofSize: 14, weight: .semibold); sidebar.addArrangedSubview(historyTitle)
        history.isEditable = false; history.font = .systemFont(ofSize: 11); history.backgroundColor = .clear
        let historyScroll = NSScrollView(); historyScroll.documentView = history; historyScroll.hasVerticalScroller = true; sidebar.addArrangedSubview(historyScroll)
        historyScroll.widthAnchor.constraint(equalToConstant: 244).isActive = true; historyScroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
        for view in [header, canvas, sidebar, status] { content.addSubview(view); view.translatesAutoresizingMaskIntoConstraints = false }
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12), header.topAnchor.constraint(equalTo: content.topAnchor, constant: 10),
            sidebar.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12), sidebar.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 14), sidebar.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -10),
            canvas.leadingAnchor.constraint(equalTo: content.leadingAnchor), canvas.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 12), canvas.trailingAnchor.constraint(equalTo: sidebar.leadingAnchor, constant: -12), canvas.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -8),
            status.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12), status.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -8)
        ])
        canvas.onError = { [weak self] error in self?.showError(error) }
        canvas.onCommitted = { [weak self] in self?.refresh() }
        window.center(); refresh()
    }
    required init?(coder: NSCoder) { nil }
    func handle(_ event: EngineEventDto) {
        switch event {
        case .documentOpened: canvas.markDirty()
        case .stateChanged(let json):
            if let new = try? EditorSnapshot.decode(json), new.revision >= (state?.revision ?? 0) { apply(new) }
        }
    }
    private func refresh() { do { apply(try EditorSnapshot.decode(engine.snapshotJson())) } catch { showError(error) } }
    private func apply(_ snapshot: EditorSnapshot) {
        refreshing = true; defer { refreshing = false }
        state = snapshot; window?.isDocumentEdited = snapshot.dirty
        if let path = engine.projectPath() { fileName = URL(fileURLWithPath: path).lastPathComponent }
        window?.title = "\(fileName) — Strata"
        if let frame = snapshot.frame { window?.subtitle = "\(frame.width) × \(frame.height) · \(snapshot.profileName ?? "RGB") · \(snapshot.bitDepth ?? 8) 位" }
        table.reloadData()
        if let id = snapshot.activeLayer {
            let rows = Array(snapshot.layers.reversed()); if let row = rows.firstIndex(where: { $0.id == id }) { table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false) }
        } else { table.deselectAll(nil) }
        updateSelection()
        let edits = snapshot.history.enumerated().map { index, item in "\(index < snapshot.historyCursor ? "✓" : "↪") \(item.command) · \(item.source)" }
        let files = snapshot.executions.map { "文件 · \($0.command) · \($0.status)（不随撤销恢复）" }
        history.string = (edits + files).joined(separator: "\n")
        status.stringValue = snapshot.profileFallback ? "ICC 无效，按 sRGB 处理 · 原始信息已保留" : "\(snapshot.dirty ? "有未保存修改" : "无未保存修改") · \(snapshot.layers.count) 个图层"
        canvas.markDirty()
    }
    private var selected: EditorSnapshot.Layer? { guard let state, table.selectedRow >= 0 else { return nil }; let rows = Array(state.layers.reversed()); return rows.indices.contains(table.selectedRow) ? rows[table.selectedRow] : nil }
    private func updateSelection() {
        let layer = selected; nameField.stringValue = layer?.name ?? ""; opacity.doubleValue = (layer?.opacity ?? 1) * 100; visibility.state = layer?.visible == true ? .on : .off
        blend.selectItem(at: ["normal", "multiply", "screen", "overlay"].firstIndex(of: layer?.blendMode ?? "normal") ?? 0)
        canvas.activeLayer = layer
        for c in [nameField, opacity, visibility, blend] as [NSControl] { c.isEnabled = layer != nil && !busy }
    }
    func numberOfRows(in tableView: NSTableView) -> Int { state?.layers.count ?? 0 }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let state else { return nil }; let layer = Array(state.layers.reversed())[row]
        let field = NSTextField(labelWithString: "\(layer.visible ? "◉" : "○")  \(layer.name)"); field.lineBreakMode = .byTruncatingTail; return field
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        updateSelection(); guard !refreshing, let layer = selected else { return }; perform("layer.select", ["layer": ["id": layer.id]])
    }
    private func perform(_ command: String, _ params: [String: Any], completion: (@MainActor @Sendable (Bool) -> Void)? = nil) {
        guard !busy else { completion?(false); return }
        canvas.cancelBenchmarkForDocumentChange()
        do {
            let encoded = try commandJSON(params); busy = true; controls.forEach { $0.isEnabled = false }; table.isEnabled = false; canvas.isEditingEnabled = false; status.stringValue = "正在处理…"
            let engine = engine
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let result = Result { try engine.dispatch(command: command, paramsJson: encoded) }
                DispatchQueue.main.async {
                    guard let self else { return }; self.finishWork()
                    switch result {
                    case .success:
                        if command == "file.new" || command == "file.open" { self.canvas.fit() }
                        completion?(true)
                    case .failure(let error): self.showError(error); completion?(false)
                    }
                }
            }
        } catch { showError(error); completion?(false) }
    }
    private func finishWork() { busy = false; controls.forEach { $0.isEnabled = true }; table.isEnabled = true; canvas.isEditingEnabled = true; refresh() }
    func open(_ url: URL, completion: ((Result<ImageInfo, Error>) -> Void)? = nil) {
        canvas.cancelBenchmarkForDocumentChange()
        confirmDiscard { [weak self] approved in
            guard let self, approved else { return }
            do { let resource = try self.engine.grantRead(path: url.path)
                self.perform("file.open", ["resource": resource, "discard_changes": true]) { ok in
                    if ok { self.fileName = url.lastPathComponent; self.refresh(); if let info = self.engine.documentInfo() { completion?(.success(info)) } }
                    else { completion?(.failure(BenchmarkFailure("Open failed"))) }
                }
            } catch { self.showError(error); completion?(.failure(error)) }
        }
    }
    private func confirmDiscard(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard hasUnsavedChanges, let window else { completion(true); return }
        let alert = NSAlert(); alert.messageText = "保存当前修改？"; alert.informativeText = "未保存的修改会在关闭或替换文档后丢失。"
        alert.addButton(withTitle: "保存"); alert.addButton(withTitle: "取消"); alert.addButton(withTitle: "不保存")
        alert.beginSheetModal(for: window) { [weak self] response in
            switch response { case .alertFirstButtonReturn: self?.save(asNew: false, completion: completion)
            case .alertThirdButtonReturn: completion(true)
            default: completion(false) }
        }
    }
    @objc func openImage() {
        guard !busy, let window else { return }; let panel = NSOpenPanel(); panel.allowsMultipleSelection = false
        panel.allowedContentTypes = ["strata", "png", "jpg", "jpeg", "webp", "tiff", "tif", "gif", "bmp"].compactMap { UTType(filenameExtension: $0) }; panel.allowsOtherFileTypes = true
        panel.beginSheetModal(for: window) { [weak self] response in if response == .OK, let url = panel.url { self?.open(url) } }
    }
    @objc func newDocument() {
        guard !busy else { return }
        confirmDiscard { [weak self] ok in guard let self, ok, let window = self.window else { return }
            let alert = NSAlert(); alert.messageText = "新建文档"; alert.addButton(withTitle: "创建"); alert.addButton(withTitle: "取消")
            let width = NSTextField(string: "1024"), height = NSTextField(string: "768"); let depth = NSPopUpButton(); depth.addItems(withTitles: ["8 位", "16 位"])
            let fields = NSStackView(views: [NSTextField(labelWithString: "宽度"), width, NSTextField(labelWithString: "高度"), height, depth]); fields.orientation = .vertical; fields.frame = NSRect(x: 0, y: 0, width: 220, height: 150); alert.accessoryView = fields
            alert.beginSheetModal(for: window) { response in guard response == .alertFirstButtonReturn else { return }
                self.fileName = "未命名"; self.perform("file.new", ["width": width.integerValue, "height": height.integerValue, "bit_depth": depth.indexOfSelectedItem == 0 ? 8 : 16, "discard_changes": true])
            }
        }
    }
    @objc func saveDocument() { save(asNew: false) { _ in } }
    @objc func saveAs() { save(asNew: true) { _ in } }
    private func save(asNew: Bool, completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard state?.frame != nil, !busy, let window else { completion(false); return }
        let write: (URL, Bool) -> Void = { [weak self] url, overwrite in guard let self else { return }
            do { let resource = try self.engine.grantWrite(path: url.path, overwrite: overwrite); self.perform(asNew ? "file.save_as" : "file.save", ["resource": resource], completion: completion) }
            catch { self.showError(error); completion(false) }
        }
        if !asNew, let path = engine.projectPath() { write(URL(fileURLWithPath: path), true); return }
        let panel = NSSavePanel(); panel.nameFieldStringValue = URL(fileURLWithPath: fileName).deletingPathExtension().lastPathComponent + ".strata"
        panel.allowedContentTypes = [UTType(filenameExtension: "strata") ?? .data]; panel.canCreateDirectories = true
        panel.beginSheetModal(for: window) { response in guard response == .OK, let url = panel.url else { completion(false); return }; write(url, FileManager.default.fileExists(atPath: url.path)) }
    }
    @objc func exportDocument() {
        guard state?.frame != nil, !busy, let window else { return }; let panel = NSSavePanel(); panel.nameFieldStringValue = "导出.png"; panel.allowedContentTypes = [.png]
        let format = NSPopUpButton(); format.addItems(withTitles: ["PNG", "TIFF"]); panel.accessoryView = format
        let formatController = ExportFormatController(panel: panel)
        format.target = formatController; format.action = #selector(ExportFormatController.changeFormat(_:))
        panel.beginSheetModal(for: window) { [weak self] response in guard let self, response == .OK, let chosen = panel.url else { return }
            _ = formatController // Retain the accessory target for the sheet's lifetime.
            let kind = format.indexOfSelectedItem == 0 ? "png" : "tiff"
            // Keep the exact target confirmed by the save panel; do not silently change its path.
            do { let resource = try self.engine.grantWrite(path: chosen.path, overwrite: FileManager.default.fileExists(atPath: chosen.path)); self.perform("file.export", ["resource": resource, "format": kind]) } catch { self.showError(error) }
        }
    }
    private func layerCommand(_ command: String, _ extra: [String: Any] = [:]) { guard let layer = selected else { return }; var params = extra; params["layer"] = ["id": layer.id]; perform(command, params) }
    @objc func createLayer() {
        guard !busy, state?.frame != nil, let window else { return }
        let alert = NSAlert(); alert.messageText = "新建像素图层"; alert.addButton(withTitle: "创建"); alert.addButton(withTitle: "取消")
        let name = NSTextField(string: "图层 \((state?.layers.count ?? 0) + 1)")
        let filled = NSButton(checkboxWithTitle: "填充颜色", target: nil, action: nil)
        let maximum = state?.bitDepth == 16 ? 65535 : 255
        let channels = (0..<4).map { _ in NSTextField(string: String(maximum)) }
        let row = NSStackView(views: channels); row.spacing = 6
        let stack = NSStackView(views: [name, filled, NSTextField(labelWithString: "文档色彩空间 RGBA（0–\(maximum)）"), row]); stack.orientation = .vertical; stack.frame = NSRect(x: 0, y: 0, width: 280, height: 130); alert.accessoryView = stack
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            var params: [String: Any] = ["name": name.stringValue]
            if filled.state == .on { params["fill"] = channels.map(\.integerValue) }
            self.perform("layer.create", params)
        }
    }
    @objc func duplicateLayer() { layerCommand("layer.duplicate") }
    @objc func deleteLayer() { layerCommand("layer.delete") }
    @objc func renameLayer() { layerCommand("layer.rename", ["name": nameField.stringValue]) }
    @objc func setVisibility() { layerCommand("layer.set_visibility", ["visible": visibility.state == .on]) }
    @objc func setOpacity() { layerCommand("layer.set_opacity", ["opacity": opacity.doubleValue / 100]) }
    @objc func setBlend() { layerCommand("layer.set_blend_mode", ["blend_mode": ["normal", "multiply", "screen", "overlay"][blend.indexOfSelectedItem]]) }
    @objc func raiseLayer() { reorder(1) }
    @objc func lowerLayer() { reorder(-1) }
    private func reorder(_ delta: Int) { guard let state, let layer = selected, let index = state.layers.firstIndex(where: { $0.id == layer.id }), state.layers.indices.contains(index + delta) else { return }; layerCommand("layer.move_order", ["index": index + delta]) }
    @objc func undo() { perform("edit.undo", [:]) }
    @objc func redo() { perform("edit.redo", [:]) }
    @objc func convertToSRGB() { perform("color.convert", ["space": "srgb"]) }
    @objc func convertToP3() { perform("color.convert", ["space": "display_p3"]) }
    @objc func toggleMove(_ sender: NSButton) { canvas.movesLayer = sender.state == .on }
    @objc func fit() { canvas.fit() }
    @objc func zoomIn() { canvas.zoomBy(1.25) }
    @objc func zoomOut() { canvas.zoomBy(0.8) }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if busy { return false }; if allowClose || !hasUnsavedChanges { return true }
        confirmDiscard { [weak self] ok in if ok { self?.allowClose = true; self?.close() } }; return false
    }
    func requestTermination(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) { if busy { completion(false) } else { confirmDiscard(completion) } }
    func windowWillClose(_ notification: Notification) { canvas.shutdown() }
    func windowDidDeminiaturize(_ notification: Notification) { canvas.markDirty() }
    func windowDidChangeOcclusionState(_ notification: Notification) { canvas.visibilityChanged() }
    private func showError(_ error: Error) {
        guard let window, window.attachedSheet == nil else { return }; let alert = NSAlert(); alert.messageText = "操作未完成"
        alert.informativeText = (error as? StrataError)?.userMessage ?? error.localizedDescription; alert.beginSheetModal(for: window)
    }
}

@MainActor
private final class ExportFormatController: NSObject {
    private weak var panel: NSSavePanel?
    init(panel: NSSavePanel) { self.panel = panel }
    @objc func changeFormat(_ sender: NSPopUpButton) {
        guard let panel else { return }
        let ext = sender.indexOfSelectedItem == 0 ? "png" : "tiff"
        panel.allowedContentTypes = sender.indexOfSelectedItem == 0 ? [.png] : [.tiff]
        panel.nameFieldStringValue = (panel.nameFieldStringValue as NSString).deletingPathExtension + "." + ext
    }
}
