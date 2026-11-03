import AppKit
import UniformTypeIdentifiers

extension MainWindowController {
    func open(_ url: URL, recordRecent: Bool = true, completion: ((Result<ImageInfo, Error>) -> Void)? = nil) {
        canvas.cancelBenchmarkForDocumentChange()
        confirmDiscard { [weak self] approved in
            guard let self, approved else { return }
            do {
                let resource = try self.engine.grantRead(path: url.path)
                self.perform("file.open", ["resource": resource, "discard_changes": true]) { ok in
                    if ok {
                        self.documentURL = url.standardizedFileURL; self.fileName = url.lastPathComponent; self.refresh()
                        if recordRecent && !url.path.hasPrefix(Bundle.main.bundleURL.path + "/") { RecentFileStore().record(url) }
                        if let info = self.engine.documentInfo() { completion?(.success(info)) }
                    }
                    else { completion?(.failure(BenchmarkFailure("Open failed"))) }
                }
            } catch { self.showError(error); completion?(.failure(error)) }
        }
    }
    func confirmDiscard(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) {
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
        panel.beginSheetModal(for: window) { [weak self] response in if response == .OK, let url = panel.url { self?.openInTab(url) } }
    }
    private func openInTab(_ url: URL) { if let onOpenURL { onOpenURL(url) } else { open(url) } }
    @objc func openRecent(_ sender: NSMenuItem) { if let url = sender.representedObject as? URL { openInTab(url) } }
    @objc func placeImage() {
        guard workspace.permissions.edits, workspace.hasDocument, let window else { return }
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = false
        panel.title = "置入图片"; panel.prompt = "置入"
        panel.allowedContentTypes = [.png, .jpeg, .tiff, .gif, .bmp, .webP]
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .OK, let url = panel.url else { return }
            do { let resource = try self.engine.grantRead(path: url.path)
                self.perform("file.place", ["resource": resource, "x": 0, "y": 0])
            } catch { self.showError(error) }
        }
    }
    @objc func newDocument() {
        guard !busy else { return }
        confirmDiscard { [weak self] ok in guard let self, ok, let window = self.window else { return }
            let alert = NSAlert(); alert.messageText = "新建文档"; alert.addButton(withTitle: "创建"); alert.addButton(withTitle: "取消")
            let width = NSTextField(string: "1024"), height = NSTextField(string: "768"); let depth = NSPopUpButton(); depth.addItems(withTitles: ["8 位", "16 位"])
            let fields = NSStackView(views: [NSTextField(labelWithString: "宽度"), width, NSTextField(labelWithString: "高度"), height, depth]); fields.orientation = .vertical; fields.frame = NSRect(x: 0, y: 0, width: 220, height: 150); alert.accessoryView = fields
            alert.beginSheetModal(for: window) { response in guard response == .alertFirstButtonReturn else { return }
                self.perform("file.new", ["width": width.integerValue, "height": height.integerValue, "bit_depth": depth.indexOfSelectedItem == 0 ? 8 : 16, "discard_changes": true]) { ok in if ok { self.documentURL = nil; self.fileName = "未命名"; self.refresh() } }
            }
        }
    }
    @objc func saveDocument() { save(asNew: false) { _ in } }
    @objc func saveAs() { save(asNew: true) { _ in } }
    private func save(asNew: Bool, completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard state?.frame != nil, !busy, let window else { completion(false); return }
        let write: (URL, Bool) -> Void = { [weak self] url, overwrite in guard let self else { return }
            do { let resource = try self.engine.grantWrite(path: url.path, overwrite: overwrite)
                self.perform(asNew ? "file.save_as" : "file.save", ["resource": resource]) { ok in
                    if ok { RecentFileStore().record(url) }; completion(ok)
                }
            }
            catch { self.showError(error); completion(false) }
        }
        if !asNew, let path = engine.projectPath() { write(URL(fileURLWithPath: path), true); return }
        let panel = NSSavePanel(); panel.nameFieldStringValue = URL(fileURLWithPath: fileName).deletingPathExtension().lastPathComponent + ".strata"
        panel.allowedContentTypes = [UTType(filenameExtension: "strata") ?? .data]; panel.canCreateDirectories = true
        panel.beginSheetModal(for: window) { response in guard response == .OK, let url = panel.url else { completion(false); return }; write(url, FileManager.default.fileExists(atPath: url.path)) }
    }
    @objc func exportDocument() {
        guard state?.frame != nil, !busy, let window else { return }; let panel = NSSavePanel(); panel.nameFieldStringValue = "导出.png"; panel.allowedContentTypes = [.png]
        let options = ExportOptionsController(panel: panel, bitDepth: state?.bitDepth ?? 8)
        panel.beginSheetModal(for: window) { [weak self] response in guard let self, response == .OK, let chosen = panel.url else { return }
            do { var params = try options.parameters()
                params["resource"] = try self.engine.grantWrite(path: chosen.path, overwrite: FileManager.default.fileExists(atPath: chosen.path))
                self.perform("file.export", params)
            } catch { self.showError(error) }
        }
    }
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
}
