import AppKit
import Foundation

@MainActor
enum NativeClipboard {
    // 512 MiB image payload plus 16 MiB container/profile overhead. A typed
    // constant avoids Swift 6.1's overload search for chained integer operators.
    private static let maximumImageBytes: Int = 553_648_128
    private static let originType=NSPasteboard.PasteboardType("dev.strata-editor.clipboard-origin-v1")
    private struct Origin:Codable{let x:Int64;let y:Int64}
    static func write(_ transfer:ClipboardTransfer,pasteboard:NSPasteboard = .general)throws{
        let metadata=try JSONEncoder().encode(Origin(x:transfer.originX,y:transfer.originY))
        pasteboard.clearContents()
        guard pasteboard.setData(transfer.data,forType:.tiff),pasteboard.setData(metadata,forType:originType)else{throw AIClientError.message("系统剪贴板写入失败；源像素未剪切。")}
    }
    static func read(engine:StrataEngine,turn:String?,pasteboard:NSPasteboard = .general)async throws->String{
        guard let data=pasteboard.data(forType:.tiff) ?? pasteboard.data(forType:.png)else{throw AIClientError.message("系统剪贴板中没有支持的图片。")}
        guard data.count <= maximumImageBytes else{throw AIClientError.message("剪贴板图片超过大小限制。")}
        let originData = pasteboard.data(forType: originType)
        let origin: Origin? = originData.flatMap { bytes in
            try? JSONDecoder().decode(Origin.self, from: bytes)
        }
        let originX: Int64 = origin?.x ?? 0
        let originY: Int64 = origin?.y ?? 0
        return try await aiEngineWork { () throws -> String in
            try engine.clipboardImport(data: data, originX: originX, originY: originY, turnId: turn)
        }
    }
}
extension MainWindowController {
    @objc func copyPixels(_ sender:Any?){if let text=window?.firstResponder as? NSTextView,text.isSelectable{text.copy(sender)}else{copyOrCut(merged:false,cut:false)}}
    @objc func copyMergedPixels(_ sender:Any?){copyOrCut(merged:true,cut:false)}
    @objc func cutPixels(_ sender:Any?){if let textEditor{textEditor.cut(sender)}else{copyOrCut(merged:false,cut:true)}}
    @objc func pastePixels(_ sender:Any?){if let textEditor{textEditor.paste(sender)}else{pasteClipboard(inPlace:false)}}
    @objc func pasteInPlace(_ sender:Any?){pasteClipboard(inPlace:true)}
    @objc func selectAllPixels(_ sender:Any?){if let text=window?.firstResponder as? NSTextView,text.isSelectable{text.selectAll(sender)}else if workspace.permissions.selectionEdits{perform("select.all",[:])}}
    @objc func clearPixels(_ sender:Any?){if let textEditor{textEditor.delete(sender)}else if workspace.permissions.edits{perform("edit.clear",[:])}}
    private func copyOrCut(merged:Bool,cut:Bool){
        guard !busy,workspace.permissions.edits,workspace.hasDocument else{return}
        busy=true;let engine=engine
        Task{[weak self] in
            guard let self else{return};defer{self.busy=false;self.refresh()}
            do{
                let id=merged ? "edit.copy_merged" : "edit.copy"
                let raw=try await aiEngineWork{try engine.dispatch(command:id,paramsJson:"{}")}
                let result=try AIJSON.decode(raw)
                guard let resource=result["clipboard_resource"].string else{throw AIClientError.message("没有生成剪贴板资源。")}
                let transfer=try await aiEngineWork{try engine.clipboardExport(resource:resource,turnId:nil)}
                try Task.checkCancellation();try NativeClipboard.write(transfer)
                try engine.clipboardConfirmWrite(resource:resource,turnId:nil)
                if cut{
                    let params=try commandJSON(["resource":resource])
                    _ = try await aiEngineWork{try engine.dispatch(command:"edit.cut",paramsJson:params)}
                }
            }catch{self.showError(error)}
        }
    }
    private func pasteClipboard(inPlace:Bool){
        guard !busy,workspace.permissions.edits,workspace.hasDocument else{return}
        busy=true;let engine=engine
        Task{[weak self] in
            guard let self else{return};defer{self.busy=false;self.refresh()}
            do{
                let resource=try await NativeClipboard.read(engine:engine,turn:nil)
                let params=try commandJSON(["resource":resource])
                _ = try await aiEngineWork{try engine.dispatch(command:inPlace ? "edit.paste_in_place" : "edit.paste",paramsJson:params)}
            }catch{self.showError(error)}
        }
    }
}
