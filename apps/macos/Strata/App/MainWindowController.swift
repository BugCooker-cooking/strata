import AppKit
import SwiftUI

@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSToolbarItemValidation, NSMenuItemValidation {
    let engine: StrataEngine
    let canvas: CanvasView
    let workspace = WorkspaceModel()
    private let split = NSSplitViewController()
    private var inspectorItem: NSSplitViewItem!
    private var assistantItem: NSSplitViewItem!
    private var allowClose = false
    private var thumbnailRevision: UInt64?
    var fileName = "未命名"
    var documentURL: URL?
    var state: EditorSnapshot? { workspace.snapshot }
    var selected: EditorSnapshot.Layer? { workspace.selected }
    var busy = false {
        didSet { workspace.busy = busy; canvas.isEditingEnabled = workspace.permissions.edits; window?.toolbar?.validateVisibleItems() }
    }
    var onNewTab: (() -> Void)?
    var onOpenURL: ((URL) -> Void)?
    var onTheme: ((WorkspaceTheme) -> Void)?
    var onClosed: (() -> Void)?
    var onBecameMain: (() -> Void)?
    var hasUnsavedChanges: Bool { state?.dirty == true }
    var canTerminateNow: Bool { !busy && (allowClose || !hasUnsavedChanges) }
    func setBenchmarkRunning(_ running: Bool) { busy = running }

    init(engine: StrataEngine) {
        self.engine = engine; canvas = CanvasView(engine: engine)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1512, height: 930), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init(window: window)
        window.title = "Strata"; window.titleVisibility = .hidden
        window.minSize = NSSize(width: 1280, height: 720); window.toolbarStyle = .unifiedCompact
        window.isReleasedWhenClosed = false; window.delegate = self
        window.tabbingIdentifier = "StrataDocuments"; window.tabbingMode = .preferred
        window.appearance = workspace.theme.appearance; window.backgroundColor = WorkspaceColors.window
        let toolbar = NSToolbar(identifier: "StrataWorkspace"); toolbar.delegate = self; toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false; window.toolbar = toolbar
        let content = NSView(); window.contentView = content
        let chrome = CanvasChromeView(canvas: canvas, model: workspace)
        let options = NSHostingView(rootView: ToolOptionsBar(model: workspace))
        let tools = NSHostingView(rootView: ToolRail(model: workspace))
        let status = NSHostingView(rootView: WorkspaceStatusBar(model: workspace, readout: chrome.readout))
        let canvasController = NSViewController(); canvasController.view = chrome
        let canvasItem = NSSplitViewItem(viewController: canvasController); canvasItem.minimumThickness = 400
        inspectorItem = NSSplitViewItem(viewController: NSHostingController(rootView: WorkspaceInspector(model: workspace)))
        inspectorItem.minimumThickness = 250; inspectorItem.maximumThickness = 380; inspectorItem.canCollapse = true
        assistantItem = NSSplitViewItem(viewController: NSHostingController(rootView: AssistantPanel(model: workspace)))
        assistantItem.minimumThickness = 300; assistantItem.maximumThickness = 460; assistantItem.canCollapse = true
        split.splitView.isVertical = true; split.splitView.dividerStyle = .thin; split.splitView.autosaveName = "workspace.split.v1"
        split.addSplitViewItem(canvasItem); split.addSplitViewItem(inspectorItem); split.addSplitViewItem(assistantItem)
        for view in [options, tools, split.view, status] { content.addSubview(view); view.translatesAutoresizingMaskIntoConstraints = false }
        NSLayoutConstraint.activate([
            options.topAnchor.constraint(equalTo: content.topAnchor), options.leadingAnchor.constraint(equalTo: content.leadingAnchor), options.trailingAnchor.constraint(equalTo: content.trailingAnchor), options.heightAnchor.constraint(equalToConstant: 36),
            tools.topAnchor.constraint(equalTo: options.bottomAnchor), tools.leadingAnchor.constraint(equalTo: content.leadingAnchor), tools.widthAnchor.constraint(equalToConstant: 46), tools.bottomAnchor.constraint(equalTo: status.topAnchor),
            split.view.topAnchor.constraint(equalTo: options.bottomAnchor), split.view.leadingAnchor.constraint(equalTo: tools.trailingAnchor), split.view.trailingAnchor.constraint(equalTo: content.trailingAnchor), split.view.bottomAnchor.constraint(equalTo: status.topAnchor),
            status.leadingAnchor.constraint(equalTo: content.leadingAnchor), status.trailingAnchor.constraint(equalTo: content.trailingAnchor), status.bottomAnchor.constraint(equalTo: content.bottomAnchor), status.heightAnchor.constraint(equalToConstant: 24)
        ])
        workspace.action = { [weak self] action in self?.act(action) }
        canvas.onError = { [weak self] error in self?.showError(error) }
        canvas.onCommitted = { [weak self] in self?.refresh() }
        canvas.onToolKey = { [weak self] key in
            guard let self, let tool = EditorTool.allCases.first(where: { $0.key == key && $0.available }) else { return false }
            self.workspace.selectTool(tool); return true
        }
        window.center(); content.layoutSubtreeIfNeeded()
        split.splitView.setPosition(max(400, content.bounds.width - 46 - 272 - 344), ofDividerAt: 0)
        split.splitView.setPosition(max(650, content.bounds.width - 46 - 344), ofDividerAt: 1)
        refresh()
    }
    required init?(coder: NSCoder) { nil }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] { toolbarDefaultItemIdentifiers(toolbar) }
    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        let leading = ["document", "newTab", "open"].map { NSToolbarItem.Identifier($0) }
        let trailing = ["undo", "redo", "save", "export", "assistant", "panels"].map { NSToolbarItem.Identifier($0) }
        return leading + [NSToolbarItem.Identifier.flexibleSpace] + trailing
    }
    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: id)
        if id.rawValue == "document" {
            let view = NSHostingView(rootView: DocumentTitleView(model: workspace)); view.frame = NSRect(x: 0, y: 0, width: 230, height: 28)
            item.view = view; item.label = "当前文档"; return item
        }
        let entries: [String: (String, String, Selector)] = [
            "newTab": ("新建标签页", "plus", #selector(newTab)), "open": ("打开", "folder", #selector(openImage)),
            "undo": ("撤销", "arrow.uturn.backward", #selector(undo)), "redo": ("重做", "arrow.uturn.forward", #selector(redo)),
            "save": ("保存", "square.and.arrow.down", #selector(saveDocument)), "export": ("导出", "square.and.arrow.up", #selector(exportDocument)),
            "assistant": ("AI 助手 (⌘J)", "sparkles", #selector(toggleAssistant)), "panels": ("显示 / 收起右侧面板", "sidebar.right", #selector(togglePanels))]
        guard let entry = entries[id.rawValue] else { return nil }
        item.label = entry.0; item.toolTip = entry.0; item.image = NSImage(systemSymbolName: entry.1, accessibilityDescription: entry.0)
        item.target = self; item.action = entry.2; return item
    }
    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        switch item.itemIdentifier.rawValue {
        case "undo": canUndo
        case "redo": canRedo
        case "save", "export": workspace.permissions.edits && workspace.hasDocument
        case "open", "newTab": !busy
        default: true
        }
    }
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(undo): canUndo
        case #selector(redo): canRedo
        case #selector(saveDocument), #selector(saveAs), #selector(exportDocument), #selector(placeImage), #selector(convertToSRGB), #selector(convertToP3): workspace.permissions.edits && workspace.hasDocument
        case #selector(openImage), #selector(newTab), #selector(openRecent(_:)): !busy
        default: true
        }
    }
    private func act(_ action: WorkspaceAction) {
        switch action {
        case .new: newDocument()
        case .newTab: newTab()
        case .open: openImage()
        case .place: placeImage()
        case .save: saveDocument()
        case .export: exportDocument()
        case .undo: undo()
        case .redo: redo()
        case .fit: fit()
        case .zoom(let factor): canvas.zoomBy(factor)
        case .actualSize: canvas.actualSize()
        case .newLayer: createLayer()
        case .duplicateLayer: duplicateLayer()
        case .deleteLayer: deleteLayer()
        case .reorder(let delta): reorder(delta)
        case .selectLayer(let id): perform("layer.select", ["layer": ["id": id]])
        case .rename(let name): layerCommand("layer.rename", ["name": name])
        case .visibility(let id, let visible): perform("layer.set_visibility", ["layer": ["id": id], "visible": visible])
        case .opacity(let value): layerCommand("layer.set_opacity", ["opacity": value])
        case .blend(let value): layerCommand("layer.set_blend_mode", ["blend_mode": value])
        case .position(let x, let y): layerCommand("layer.set_position", ["x": x, "y": y])
        case .tool(let tool): canvas.movesLayer = tool == .move; canvas.zoomTool = tool == .zoom; window?.makeFirstResponder(canvas)
        case .assistant: toggleAssistant()
        case .panels: togglePanels()
        case .theme(let theme): onTheme?(theme)
        }
    }
    func applyTheme(_ theme: WorkspaceTheme) {
        workspace.theme = theme; window?.appearance = theme.appearance; canvas.markDirty()
    }
    @objc func toggleAssistant() { assistantItem.isCollapsed.toggle(); workspace.assistantVisible = !assistantItem.isCollapsed }
    @objc func togglePanels() {
        let hide = !inspectorItem.isCollapsed; inspectorItem.isCollapsed = hide; assistantItem.isCollapsed = hide
        workspace.inspectorVisible = !hide; workspace.assistantVisible = !hide
    }
    @objc func newTab() { if let onNewTab { onNewTab() } else { newDocument() } }
    func handle(_ event: EngineEventDto) {
        switch event {
        case .documentOpened: canvas.markDirty()
        case .stateChanged(let json): if let new = try? EditorSnapshot.decode(json), new.revision >= (state?.revision ?? 0) { apply(new) }
        }
    }
    func refresh() { do { apply(try EditorSnapshot.decode(engine.snapshotJson())) } catch { showError(error) } }
    private func apply(_ snapshot: EditorSnapshot) {
        let oldControl = state?.control?.kind
        workspace.snapshot = snapshot; window?.isDocumentEdited = snapshot.dirty
        if let path = engine.projectPath() { documentURL = URL(fileURLWithPath: path); fileName = documentURL!.lastPathComponent }
        workspace.fileName = fileName; window?.title = "\(fileName) — Strata"
        if thumbnailRevision != snapshot.revision {
            thumbnailRevision = snapshot.revision
            let revision = snapshot.revision, engine = engine
            DispatchQueue.global(qos: .utility).async { [weak self] in
                let previews = (try? engine.layerThumbnails(revision: revision)) ?? []
                DispatchQueue.main.async {
                    guard let self, self.thumbnailRevision == revision else { return }
                    self.workspace.thumbnails = Dictionary(uniqueKeysWithValues: previews.compactMap { preview in
                        guard let provider = CGDataProvider(data: Data(preview.rgba) as CFData),
                              let space = CGColorSpace(name: CGColorSpace.displayP3),
                              let image = CGImage(width: Int(preview.width), height: Int(preview.height), bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(preview.width) * 4, space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent) else { return nil }
                        return (preview.layerId, NSImage(cgImage: image, size: NSSize(width: Int(preview.width), height: Int(preview.height))))
                    })
                }
            }
        }
        canvas.activeLayer = selected; canvas.isEditingEnabled = workspace.permissions.edits
        if snapshot.control?.kind != oldControl, snapshot.control?.kind != "user" { workspace.inspector = .history }
        window?.toolbar?.validateVisibleItems(); canvas.markDirty()
    }
    func perform(_ command: String, _ params: [String: Any], completion: (@MainActor @Sendable (Bool) -> Void)? = nil) {
        guard !busy else { completion?(false); return }
        canvas.cancelBenchmarkForDocumentChange()
        do {
            let encoded = try commandJSON(params); busy = true; let engine = engine
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let result = Result { try engine.dispatch(command: command, paramsJson: encoded) }
                DispatchQueue.main.async {
                    guard let self else { return }; self.busy = false; self.refresh()
                    switch result {
                    case .success: if command == "file.new" || command == "file.open" { self.canvas.fit() }; completion?(true)
                    case .failure(let error): self.showError(error); completion?(false)
                    }
                }
            }
        } catch { showError(error); completion?(false) }
    }
    func layerCommand(_ command: String, _ extra: [String: Any] = [:]) { guard let layer = selected else { return }; var params = extra; params["layer"] = ["id": layer.id]; perform(command, params) }
    @objc func duplicateLayer() { layerCommand("layer.duplicate") }
    @objc func deleteLayer() { layerCommand("layer.delete") }
    private func reorder(_ delta: Int) { guard let state, let layer = selected, let index = state.layers.firstIndex(where: { $0.id == layer.id }), state.layers.indices.contains(index + delta) else { return }; layerCommand("layer.move_order", ["index": index + delta]) }
    private var textEditor: NSTextView? { (window?.firstResponder as? NSTextView).flatMap { $0.isEditable ? $0 : nil } }
    private var canUndo: Bool { textEditor.map { $0.undoManager?.canUndo == true } ?? workspace.canUndo }
    private var canRedo: Bool { textEditor.map { $0.undoManager?.canRedo == true } ?? workspace.canRedo }
    @objc func undo() { if let textEditor { textEditor.undoManager?.undo() } else if workspace.canUndo { perform("edit.undo", [:]) } }
    @objc func redo() { if let textEditor { textEditor.undoManager?.redo() } else if workspace.canRedo { perform("edit.redo", [:]) } }
    @objc func convertToSRGB() { perform("color.convert", ["space": "srgb"]) }
    @objc func convertToP3() { perform("color.convert", ["space": "display_p3"]) }
    @objc func fit() { canvas.fit() }
    @objc func zoomIn() { canvas.zoomBy(1.25) }
    @objc func zoomOut() { canvas.zoomBy(0.8) }
    @objc func actualSize() { canvas.actualSize() }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if busy { return false }; if allowClose || !hasUnsavedChanges { return true }
        confirmDiscard { [weak self] ok in if ok { self?.allowClose = true; self?.close() } }; return false
    }
    func requestTermination(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) { if busy { completion(false) } else { confirmDiscard(completion) } }
    func windowWillClose(_ notification: Notification) { canvas.shutdown(); onClosed?() }
    func windowDidBecomeMain(_ notification: Notification) { onBecameMain?() }
    func windowDidDeminiaturize(_ notification: Notification) { canvas.markDirty() }
    func windowDidChangeOcclusionState(_ notification: Notification) { canvas.visibilityChanged() }
    func showError(_ error: Error) {
        guard let window, window.attachedSheet == nil else { return }; let alert = NSAlert(); alert.messageText = "操作未完成"
        alert.informativeText = (error as? StrataError)?.userMessage ?? error.localizedDescription; alert.beginSheetModal(for: window)
    }
}
