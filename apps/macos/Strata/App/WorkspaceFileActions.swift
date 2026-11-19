import AppKit
import UniformTypeIdentifiers

enum WorkspaceFileTypes {
    static let images: [UTType] = [.png, .jpeg, .webP, .tiff, .heic, .heif, .gif, .bmp]
    static let project = UTType(filenameExtension: "strata") ?? .data
    static let documents = images + [project] + [UTType(filenameExtension: "psd")].compactMap { $0 }
}

extension MainWindowController {
    func syncRecentFiles() { do { try engine.setRecentFiles(paths: RecentFileStore().urls.map(\.path)) } catch { showError(error) } }
    func open(_ url: URL, recordRecent: Bool = true, completion: ((Result<ImageInfo, Error>) -> Void)? = nil) {
        canvas.cancelBenchmarkForDocumentChange()
        confirmDiscard { [weak self] approved in
            guard let self, approved else { return }
            do {
                let resource = try self.engine.grantRead(path: url.path)
                self.perform("file.open", ["resource": resource, "discard_changes": true]) { ok in
                    if ok {
                        self.documentURL = url.standardizedFileURL; self.fileName = url.lastPathComponent; self.refresh()
                        if recordRecent && !url.path.hasPrefix(Bundle.main.bundleURL.path + "/") { RecentFileStore().record(url); self.syncRecentFiles() }
                        if let info = self.engine.documentInfo() { completion?(.success(info)) }
                    }
                    else { completion?(.failure(BenchmarkFailure("Open failed"))) }
                }
            } catch { self.showError(error); completion?(.failure(error)) }
        }
    }
    func confirmDiscard(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard hasUnsavedChanges, let window else { completion(true); return }
        let alert = NSAlert(); alert.messageText = L10n.text("保存当前修改？"); alert.informativeText = L10n.text("未保存的修改会在关闭或替换文档后丢失。")
        alert.addButton(withTitle: L10n.text("保存")); alert.addButton(withTitle: L10n.text("取消")); alert.addButton(withTitle: L10n.text("不保存"))
        alert.beginSheetModal(for: window) { [weak self] response in
            switch response { case .alertFirstButtonReturn: self?.save(asNew: false, completion: completion)
            case .alertThirdButtonReturn: completion(true)
            default: completion(false) }
        }
    }
    @objc func openImage() {
        guard !busy, let window else { return }; let panel = NSOpenPanel(); panel.allowsMultipleSelection = false
        panel.allowedContentTypes = WorkspaceFileTypes.documents
        panel.allowsOtherFileTypes = true
        panel.beginSheetModal(for: window) { [weak self] response in if response == .OK, let url = panel.url { self?.openInTab(url) } }
    }
    private func openInTab(_ url: URL) { if let onOpenURL { onOpenURL(url) } else { open(url) } }
    @objc func openRecent(_ sender: NSMenuItem) { if let url = sender.representedObject as? URL { openInTab(url) } }
    @objc func placeImage() {
        guard workspace.permissions.edits, workspace.hasDocument, let window else { return }
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = false
        panel.title = L10n.text("置入图片"); panel.prompt = L10n.text("置入")
        panel.allowedContentTypes = WorkspaceFileTypes.images
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
            let alert = NSAlert(); alert.messageText = L10n.text("新建文档"); alert.addButton(withTitle: L10n.text("创建")); alert.addButton(withTitle: L10n.text("取消"))
            let options = NewDocumentOptionsController(alert: alert)
            alert.beginSheetModal(for: window) { response in guard response == .alertFirstButtonReturn else { return }
                do { self.perform("file.new", try options.parameters()) { ok in if ok { self.documentURL = nil; self.fileName = L10n.text("未命名"); self.refresh() } } }
                catch { self.showError(error) }
            }
        }
    }
    @objc func saveDocument() { save(asNew: false) { _ in } }
    @objc func saveAs() { save(asNew: true) { _ in } }
    @objc func duplicateDocument() {
        guard workspace.permissions.edits, onDocumentDuplicated != nil,
              let window, let snapshot = state, snapshot.documentId != nil else { return }

        let alert = NSAlert()
        alert.messageText = L10n.text("复制文档")
        alert.informativeText = L10n.text("将当前文档复制到独立的未保存标签页。")
        alert.addButton(withTitle: L10n.text("创建副本"))
        alert.addButton(withTitle: L10n.text("取消"))

        let stem = documentURL?.deletingPathExtension().lastPathComponent ?? fileName
        let title = String(decoding: stem.utf8.prefix(240), as: UTF8.self) + L10n.text(" 副本")
        let name = NSTextField(string: title)
        name.setAccessibilityLabel(L10n.text("副本名称"))
        name.widthAnchor.constraint(equalToConstant: 320).isActive = true
        let merged = NSButton(checkboxWithTitle: L10n.text("仅复制画板内的可见合成"), target: nil, action: nil)
        let options = NSStackView(views: [name, merged])
        options.orientation = .vertical
        options.alignment = .leading
        options.spacing = 8
        options.frame = NSRect(x: 0, y: 0, width: 320, height: 60)
        alert.accessoryView = options
        alert.window.initialFirstResponder = name
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .alertFirstButtonReturn else { return }
            self.perform("image.duplicate", [
                "name": name.stringValue,
                "mode": merged.state == .on ? "merged" : "layered",
                "expected_revision": snapshot.revision
            ])
        }
    }
    @objc func revertDocument(){
        guard workspace.permissions.edits,let window,let target=engine.revertSourcePath(),let initial=state else{return}
        let alert=NSAlert();alert.messageText=L10n.text("恢复到上次保存？");alert.informativeText=L10n.format("将重新读取{0}，放弃当前修改并清除文档撤销/重做历史。磁盘原文件不变。", String(describing: URL(fileURLWithPath:target).lastPathComponent));alert.addButton(withTitle:L10n.text("恢复原文件"));alert.addButton(withTitle:L10n.text("取消"))
        alert.beginSheetModal(for:window){[weak self]response in
            guard let self,response == .alertFirstButtonReturn else{return}
            guard self.state?.documentId==initial.documentId,self.state?.revision==initial.revision,self.engine.revertSourcePath()==target else{self.showError(QuickExportError(L10n.text("当前文档已改变，请重新发起恢复。")));return}
            do{let resource=try self.engine.grantRead(path:target);self.perform("file.revert",["resource":resource,"discard_changes":true,"expected_revision":initial.revision])}
            catch{self.showError(error)}
        }
    }
    private func save(asNew: Bool, completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard state?.frame != nil, !busy, let window else { completion(false); return }
        let write: (URL, Bool) -> Void = { [weak self] url, overwrite in guard let self else { return }
            do { let resource = try self.engine.grantWrite(path: url.path, overwrite: overwrite)
                self.perform(asNew ? "file.save_as" : "file.save", ["resource": resource]) { ok in
                    if ok { RecentFileStore().record(url); self.syncRecentFiles() }; completion(ok)
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
        guard state?.frame != nil, !busy, let window else { return }; let panel = NSSavePanel(); panel.nameFieldStringValue = L10n.text("导出.png"); panel.allowedContentTypes = [.png]
        let options = ExportOptionsController(panel: panel, bitDepth: state?.bitDepth ?? 8, width: state?.frame?.width ?? 1, height: state?.frame?.height ?? 1)
        panel.beginSheetModal(for: window) { [weak self] response in guard let self, response == .OK, let chosen = panel.url else { return }
            do { var params = try options.parameters()
                params["resource"] = try self.engine.grantWrite(path: chosen.path, overwrite: FileManager.default.fileExists(atPath: chosen.path))
                self.perform("file.export", params)
            } catch { self.showError(error) }
        }
    }
    @objc func exportLayeredPSD() {
        guard workspace.permissions.edits, workspace.hasDocument, let window else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "psd") ?? .data]
        panel.nameFieldStringValue = URL(fileURLWithPath: fileName).deletingPathExtension().lastPathComponent + ".psd"
        panel.message = L10n.text("导出可编辑的 RGB 图层、组和蒙版。选区和参考线保存在 Strata 工程中。")
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .OK, let url = panel.url else { return }
            self.writeLayeredPSD(to: url)
        }
    }
    func writeLayeredPSD(to url: URL, completion: (@MainActor @Sendable (Bool) -> Void)? = nil) {
        do {
            let output = try engine.grantWrite(path: url.path, overwrite: FileManager.default.fileExists(atPath: url.path))
            perform("file.export_psd", ["resource": output], completion: completion)
        } catch {
            showError(error)
            completion?(false)
        }
    }
    @objc func configureQuickExport(){
        guard !busy,(state?.control?.kind ?? "user")=="user",let window else{return}
        do{
            let preset=try QuickExportPreferences().preset(engine)
            let options=ExportOptionsController(bitDepth:state?.bitDepth ?? 8,width:state?.frame?.width ?? 1,height:state?.frame?.height ?? 1,presetEditing:true)
            try options.loadPreset(preset.options)
            let alert=NSAlert();alert.messageText=L10n.text("快速导出预设");alert.informativeText=L10n.text("跨文档记住输出设置。快速导出时选择目标；原文档不变，导出文件不受撤销保护。");alert.accessoryView=options.view;alert.addButton(withTitle:L10n.text("保存预设"));alert.addButton(withTitle:L10n.text("取消"))
            alert.beginSheetModal(for:window){[weak self]response in
                guard let self,response == .alertFirstButtonReturn else{return}
                do{let values=try options.parameters(requireQuantization:false);self.perform("file.export_preset.set",["options":values,"expected_version":preset.version])}
                catch{self.showError(error)}
            }
        }catch{showError(error)}
    }
    @objc func quickExportDocument(){
        guard workspace.permissions.edits,workspace.hasDocument,let window else{return}
        do{
            guard quickExportError.isEmpty else{throw QuickExportError(quickExportError)}
            let preset=try QuickExportPreferences().preset(engine),format=try QuickExportPreferences().format(preset)
            if state?.bitDepth==16,["jpeg","webp"].contains(format),!preset.options["allow_quantization"].bool{throw QuickExportError(L10n.text("当前预设没有允许16位副本量化为8位。请在快速导出预设中明确允许，或改用PNG/TIFF；原文档不变。"))}
            let panel=NSSavePanel();panel.allowedContentTypes=[UTType(filenameExtension:format) ?? .data];panel.canCreateDirectories=true
            panel.nameFieldStringValue=URL(fileURLWithPath:fileName).deletingPathExtension().lastPathComponent+"."+(format=="jpeg" ? "jpg":format)
            panel.message=L10n.format("按已保存的{0}预设导出副本；源文档不变。", String(describing: format.uppercased()))
            panel.beginSheetModal(for:window){[weak self]response in
                guard let self,response == .OK,let url=panel.url else{return}
                do{
                    let resource=try self.engine.grantWrite(path:url.path,overwrite:FileManager.default.fileExists(atPath:url.path))
                    self.perform("file.quick_export",["resource":resource,"preset_version":preset.version])
                }catch{self.showError(error)}
            }
        }catch{showError(error)}
    }
    @objc func createLayer() {
        guard !busy, state?.frame != nil, let window else { return }
        let alert = NSAlert(); alert.messageText = L10n.text("新建像素图层"); alert.addButton(withTitle: L10n.text("创建")); alert.addButton(withTitle: L10n.text("取消"))
        let name = NSTextField(string: L10n.format("图层 {0}", String(describing: (state?.layers.count ?? 0) + 1)))
        let filled = NSButton(checkboxWithTitle: L10n.text("填充颜色"), target: nil, action: nil)
        let maximum = state?.bitDepth == 16 ? 65535 : 255
        let channels = (0..<4).map { _ in NSTextField(string: String(maximum)) }
        let row = NSStackView(views: channels); row.spacing = 6
        let stack = NSStackView(views: [name, filled, NSTextField(labelWithString: L10n.format("文档色彩空间 RGBA（0–{0}）", String(describing: maximum))), row]); stack.orientation = .vertical; stack.frame = NSRect(x: 0, y: 0, width: 280, height: 130); alert.accessoryView = stack
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            var params: [String: Any] = ["name": name.stringValue]
            if filled.state == .on { params["fill"] = channels.map(\.integerValue) }
            self.perform("layer.create", params)
        }
    }
}
