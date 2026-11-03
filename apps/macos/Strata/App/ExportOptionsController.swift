import AppKit
import UniformTypeIdentifiers

/// One source for the panel's visible format and the command payload.
@MainActor
final class ExportOptionsController: NSObject, NSOpenSavePanelDelegate, NSTextFieldDelegate {
    private weak var panel: NSSavePanel?
    let format = NSPopUpButton()
    let space = NSPopUpButton()
    let quality = NSTextField(string: "90")
    let background = NSPopUpButton()
    let webpMode = NSPopUpButton()
    let width = NSTextField()
    let height = NSTextField()
    let lockAspect = NSButton(checkboxWithTitle: "保持原图比例", target: nil, action: nil)
    private let sourceWidth: UInt32
    private let sourceHeight: UInt32
    let quantization = NSButton(checkboxWithTitle: "允许此副本从 16 位量化为 8 位", target: nil, action: nil)
    private let warning = NSTextField(wrappingLabelWithString: "")
    private let bitDepth: UInt8
    private let kinds = ["png", "tiff", "jpeg", "webp"]
    private let extensions = ["png", "tiff", "jpg", "webp"]
    private let types: [UTType] = [.png, .tiff, .jpeg, .webP]

    init(panel: NSSavePanel, bitDepth: UInt8, width: UInt32 = 1, height: UInt32 = 1) {
        self.panel = panel; self.bitDepth = bitDepth
        sourceWidth = width; sourceHeight = height
        super.init()
        self.width.stringValue = String(width); self.height.stringValue = String(height)
        self.width.delegate = self; self.height.delegate = self; lockAspect.state = .on
        self.width.setAccessibilityLabel("导出宽度（像素）"); self.height.setAccessibilityLabel("导出高度（像素）")
        format.addItems(withTitles: ["PNG", "TIFF", "JPEG（有损）", "WebP（无损）"])
        space.addItems(withTitles: ["保留文档工作色域", "转换为 sRGB", "转换为 Display P3"])
        background.addItems(withTitles: ["白色", "黑色"])
        webpMode.addItems(withTitles: ["无损", "有损"]); webpMode.setAccessibilityLabel("WebP 编码模式")
        quality.setAccessibilityLabel("JPEG 质量（1–100）")
        format.setAccessibilityLabel("导出格式"); space.setAccessibilityLabel("输出色域")
        background.setAccessibilityLabel("JPEG 透明区域背景")
        func row(_ title: String, _ view: NSView) -> NSView {
            let label = NSTextField(labelWithString: title); label.widthAnchor.constraint(equalToConstant: 84).isActive = true
            let row = NSStackView(views: [label, view]); row.spacing = 8; return row
        }
        warning.font = .systemFont(ofSize: 11); warning.textColor = .secondaryLabelColor
        let size = NSStackView(views: [self.width, NSTextField(labelWithString: "×"), self.height, NSTextField(labelWithString: "px")]); size.spacing = 6
        let stack = NSStackView(views: [row("格式", format), row("输出尺寸", size), lockAspect, row("输出色域", space), row("WebP 编码", webpMode), row("有损质量", quality), row("透明区域", background), quantization, warning])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 12, right: 16)
        stack.frame = NSRect(x: 0, y: 0, width: 380, height: 320)
        stack.widthAnchor.constraint(equalToConstant: 380).isActive = true
        warning.widthAnchor.constraint(equalToConstant: 348).isActive = true
        panel.accessoryView = stack; panel.delegate = self
        format.target = self; format.action = #selector(changeFormat(_:)); update()
        lockAspect.target = self; lockAspect.action = #selector(changeAspect)
        webpMode.target = self; webpMode.action = #selector(changeEncoding)
    }
    @objc func changeFormat(_ sender: NSPopUpButton) { update() }
    @objc func changeEncoding() { update() }
    @objc func changeAspect() {
        if lockAspect.state == .on { controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: width)) }
    }
    func controlTextDidChange(_ notification: Notification) {
        if lockAspect.state == .on, let field = notification.object as? NSTextField,
           let value = UInt32(field.stringValue), value > 0, sourceWidth > 0, sourceHeight > 0 {
            let scaled = field === width ? Double(value) * Double(sourceHeight) / Double(sourceWidth) : Double(value) * Double(sourceWidth) / Double(sourceHeight)
            let rounded = max(1, scaled.rounded())
            if rounded <= Double(UInt32.max) { (field === width ? height : width).stringValue = String(UInt32(rounded)) }
        }
        updateWarning()
    }
    private func update() {
        let index = format.indexOfSelectedItem
        panel?.allowedContentTypes = [types[index]]
        if let panel { panel.nameFieldStringValue = (panel.nameFieldStringValue as NSString).deletingPathExtension + "." + extensions[index] }
        quality.isEnabled = index == 2 || index == 3 && webpMode.indexOfSelectedItem == 1
        quality.setAccessibilityLabel(index == 3 ? "WebP 质量（1–100）" : "JPEG 质量（1–100）")
        background.isEnabled = index == 2; webpMode.isEnabled = index == 3
        quantization.isEnabled = bitDepth == 16 && index >= 2
        if !quantization.isEnabled { quantization.state = .off }
        updateWarning()
    }
    private func updateWarning() {
        let index = format.indexOfSelectedItem
        warning.stringValue = index == 2
            ? "JPEG 不保留透明度；按所选背景合成，质量越高文件通常越大。文档原件不变。"
            : index == 3 ? (webpMode.indexOfSelectedItem == 0 ? "WebP 无损编码，保留透明度，仅支持 8 位输出。" : "WebP 有损压缩 RGB，透明度保持无损，仅支持 8 位输出。") + "文档原件不变。"
            : "输出保持 \(bitDepth) 位和透明度，并嵌入所选 ICC。调整尺寸会重采样，原文档不变。"
    }
    func parameters() throws -> [String: Any] {
        let index = format.indexOfSelectedItem
        if bitDepth == 16 && index >= 2 && quantization.state != .on {
            throw ExportOptionsError("此格式只支持 8 位。请勾选允许量化，或使用 PNG/TIFF 保留 16 位。")
        }
        var result: [String: Any] = ["format": kinds[index]]
        guard let w = UInt32(width.stringValue), let h = UInt32(height.stringValue), w > 0, h > 0,
              UInt64(w) * UInt64(h) <= (512 * 1024 * 1024) / UInt64(bitDepth == 16 ? 8 : 4) else {
            throw ExportOptionsError("宽高须为正整数，输出像素数据不能超过 512 MiB。")
        }
        if w != sourceWidth || h != sourceHeight { result["width"] = w; result["height"] = h }
        if index == 3 && (w > 16383 || h > 16383) { throw ExportOptionsError("WebP 宽高不能超过 16383 像素。") }
        if space.indexOfSelectedItem > 0 { result["target_space"] = space.indexOfSelectedItem == 1 ? "srgb" : "display_p3" }
        if quality.isEnabled {
            guard let value = Int(quality.stringValue), (1...100).contains(value) else { throw ExportOptionsError("有损质量须为 1–100 的整数。") }
            result["quality"] = value
        }
        if index == 3 && webpMode.indexOfSelectedItem == 1 { result["webp_lossless"] = false }
        if index == 2 {
            result["background"] = Array(repeating: background.indexOfSelectedItem == 0 ? 255 : 0, count: 3)
        }
        if bitDepth == 16 && index >= 2 { result["allow_quantization"] = true }
        return result
    }
    func panel(_ sender: Any, validate url: URL) throws { _ = try parameters() }
}

private struct ExportOptionsError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
