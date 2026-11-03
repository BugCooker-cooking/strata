import AppKit
import UniformTypeIdentifiers

/// One source for the panel's visible format and the command payload.
@MainActor
final class ExportOptionsController: NSObject, NSOpenSavePanelDelegate {
    private weak var panel: NSSavePanel?
    let format = NSPopUpButton()
    let space = NSPopUpButton()
    let quality = NSTextField(string: "90")
    let background = NSPopUpButton()
    let quantization = NSButton(checkboxWithTitle: "允许此副本从 16 位量化为 8 位", target: nil, action: nil)
    private let warning = NSTextField(wrappingLabelWithString: "")
    private let bitDepth: UInt8
    private let kinds = ["png", "tiff", "jpeg", "webp"]
    private let extensions = ["png", "tiff", "jpg", "webp"]
    private let types: [UTType] = [.png, .tiff, .jpeg, .webP]

    init(panel: NSSavePanel, bitDepth: UInt8) {
        self.panel = panel; self.bitDepth = bitDepth
        super.init()
        format.addItems(withTitles: ["PNG", "TIFF", "JPEG（有损）", "WebP（无损）"])
        space.addItems(withTitles: ["保留文档工作色域", "转换为 sRGB", "转换为 Display P3"])
        background.addItems(withTitles: ["白色", "黑色"])
        quality.setAccessibilityLabel("JPEG 质量（1–100）")
        format.setAccessibilityLabel("导出格式"); space.setAccessibilityLabel("输出色域")
        background.setAccessibilityLabel("JPEG 透明区域背景")
        func row(_ title: String, _ view: NSView) -> NSView {
            let label = NSTextField(labelWithString: title); label.widthAnchor.constraint(equalToConstant: 84).isActive = true
            let row = NSStackView(views: [label, view]); row.spacing = 8; return row
        }
        warning.font = .systemFont(ofSize: 11); warning.textColor = .secondaryLabelColor
        let stack = NSStackView(views: [row("格式", format), row("输出色域", space), row("JPEG 质量", quality), row("透明区域", background), quantization, warning])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 12, right: 16)
        stack.frame = NSRect(x: 0, y: 0, width: 380, height: 220)
        stack.widthAnchor.constraint(equalToConstant: 380).isActive = true
        warning.widthAnchor.constraint(equalToConstant: 348).isActive = true
        panel.accessoryView = stack; panel.delegate = self
        format.target = self; format.action = #selector(changeFormat(_:)); update()
    }
    @objc func changeFormat(_ sender: NSPopUpButton) { update() }
    private func update() {
        let index = format.indexOfSelectedItem
        panel?.allowedContentTypes = [types[index]]
        if let panel { panel.nameFieldStringValue = (panel.nameFieldStringValue as NSString).deletingPathExtension + "." + extensions[index] }
        quality.isEnabled = index == 2; background.isEnabled = index == 2
        quantization.isEnabled = bitDepth == 16 && index >= 2
        if !quantization.isEnabled { quantization.state = .off }
        warning.stringValue = index == 2
            ? "JPEG 不保留透明度；按所选背景合成，质量越高文件通常越大。文档原件不变。"
            : index == 3 ? "WebP 使用无损编码，保留透明度，仅支持 8 位输出。文档原件不变。"
            : "保留文档的 \(bitDepth) 位样本和透明度，并嵌入所选 ICC。导出不修改文档。"
    }
    func parameters() throws -> [String: Any] {
        let index = format.indexOfSelectedItem
        if bitDepth == 16 && index >= 2 && quantization.state != .on {
            throw ExportOptionsError("此格式只支持 8 位。请勾选允许量化，或使用 PNG/TIFF 保留 16 位。")
        }
        var result: [String: Any] = ["format": kinds[index]]
        if space.indexOfSelectedItem > 0 { result["target_space"] = space.indexOfSelectedItem == 1 ? "srgb" : "display_p3" }
        if index == 2 {
            guard let value = Int(quality.stringValue), (1...100).contains(value) else { throw ExportOptionsError("JPEG 质量须为 1–100 的整数。") }
            result["quality"] = value
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
