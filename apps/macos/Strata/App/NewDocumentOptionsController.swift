import AppKit

@MainActor
final class NewDocumentOptionsController: NSObject, NSTextFieldDelegate {
    let preset = NSPopUpButton()
    let width = NSTextField(string: "1024")
    let height = NSTextField(string: "768")
    let resolution = NSTextField(string: "72")
    let depth = NSPopUpButton()
    let space = NSPopUpButton()
    let background = NSPopUpButton()
    let channels = (0..<4).map { _ in NSTextField(string: "255") }
    private let error = NSTextField(wrappingLabelWithString: "")
    private weak var createButton: NSButton?
    private var previousDepth = 8
    private let presets: [(String, UInt32, UInt32, UInt32)] = [
        ("web_1080p", 1920, 1080, 72), ("a4_300dpi", 2480, 3508, 300),
        ("photo_4x6_300dpi", 1200, 1800, 300), ("square_2048", 2048, 2048, 72)
    ]
    init(alert: NSAlert) {
        super.init()
        createButton = alert.buttons.first
        preset.addItems(withTitles: [L10n.text("自定义"), L10n.text("屏幕 · 1920 × 1080"), "A4 · 300 dpi", L10n.text("照片 4 × 6 英寸 · 300 dpi"), L10n.text("方形 · 2048 × 2048")])
        depth.addItems(withTitles: [L10n.text("8 位"), L10n.text("16 位")])
        space.addItems(withTitles: ["sRGB", "Display P3"])
        background.addItems(withTitles: [L10n.text("透明"), L10n.text("白色"), L10n.text("黑色"), L10n.text("自定义 RGBA")])
        for (field, label) in [(width, L10n.text("文档宽度（像素）")), (height, L10n.text("文档高度（像素）")), (resolution, L10n.text("分辨率（dpi）"))] { field.delegate = self; field.setAccessibilityLabel(label) }
        for (index, field) in channels.enumerated() { field.delegate = self; field.setAccessibilityLabel([L10n.text("背景红色"), L10n.text("背景绿色"), L10n.text("背景蓝色"), L10n.text("背景透明度")][index]) }
        preset.setAccessibilityLabel(L10n.text("新建文档预设")); depth.setAccessibilityLabel(L10n.text("文档位深")); space.setAccessibilityLabel(L10n.text("文档工作色域")); background.setAccessibilityLabel(L10n.text("文档背景"))
        func row(_ label: String, _ view: NSView) -> NSView {
            let title = NSTextField(labelWithString: label)
            title.widthAnchor.constraint(equalToConstant: L10n.language == .english ? 110 : 76).isActive = true
            let row = NSStackView(views: [title, view]); row.spacing = 10; return row
        }
        let rgba = NSStackView(views: channels); rgba.spacing = 6
        error.font = .systemFont(ofSize: 11); error.textColor = .secondaryLabelColor
        error.widthAnchor.constraint(equalToConstant: 380).isActive = true
        let stack = NSStackView(views: [row(L10n.text("预设"), preset), row(L10n.text("宽度 px"), width), row(L10n.text("高度 px"), height), row(L10n.text("分辨率 dpi"), resolution), row(L10n.text("位深"), depth), row(L10n.text("工作色域"), space), row(L10n.text("背景"), background), row("RGBA", rgba), error])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10
        stack.frame = NSRect(x: 0, y: 0, width: 400, height: 320)
        stack.widthAnchor.constraint(equalToConstant: 400).isActive = true
        alert.accessoryView = stack
        preset.target = self; preset.action = #selector(changePreset)
        depth.target = self; depth.action = #selector(changeDepth)
        background.target = self; background.action = #selector(changeBackground)
        updateValidation()
    }
    @objc func changePreset() {
        if preset.indexOfSelectedItem > 0 {
            let item = presets[preset.indexOfSelectedItem - 1]
            width.stringValue = String(item.1); height.stringValue = String(item.2); resolution.stringValue = String(item.3)
        }
        updateValidation()
    }
    @objc func changeDepth() {
        let next = depth.indexOfSelectedItem == 0 ? 8 : 16, oldMax = previousDepth == 8 ? 255 : 65535, nextMax = next == 8 ? 255 : 65535
        for field in channels { if let value = Int(field.stringValue), (0...oldMax).contains(value) { field.stringValue = String((value * nextMax + oldMax / 2) / oldMax) } }
        previousDepth = next; updateValidation()
    }
    @objc func changeBackground() { updateValidation() }
    func controlTextDidChange(_ notification: Notification) {
        if let field = notification.object as? NSTextField, field === width || field === height || field === resolution { preset.selectItem(at: 0) }
        updateValidation()
    }
    private func updateValidation() {
        channels.forEach { $0.isEnabled = background.indexOfSelectedItem == 3 }
        do { _ = try parameters(); error.stringValue = L10n.text("尺寸为像素；分辨率影响打印尺寸。像素数据最多 512 MiB。"); createButton?.isEnabled = true }
        catch { self.error.stringValue = error.localizedDescription; createButton?.isEnabled = false }
    }
    func parameters() throws -> [String: Any] {
        let bits = depth.indexOfSelectedItem == 0 ? 8 : 16
        guard let w = UInt32(width.stringValue), let h = UInt32(height.stringValue), w > 0, h > 0,
              UInt64(w) * UInt64(h) <= (512 * 1024 * 1024) / UInt64(bits == 16 ? 8 : 4) else { throw NewDocumentOptionsError(L10n.text("宽高须为正整数，像素数据不能超过 512 MiB。")) }
        guard let dpi = UInt32(resolution.stringValue), (1...2400).contains(dpi) else { throw NewDocumentOptionsError(L10n.text("分辨率须为 1–2400 的整数 dpi。")) }
        var result: [String: Any] = ["width": w, "height": h, "bit_depth": bits, "resolution_dpi": dpi, "working_space": space.indexOfSelectedItem == 0 ? "srgb" : "display_p3", "discard_changes": true]
        if preset.indexOfSelectedItem > 0 { let item = presets[preset.indexOfSelectedItem - 1]; if (w, h, dpi) == (item.1, item.2, item.3) { result["preset"] = item.0 } }
        let max = bits == 8 ? 255 : 65535
        switch background.indexOfSelectedItem {
        case 1: result["background"] = [max, max, max, max]
        case 2: result["background"] = [0, 0, 0, max]
        case 3:
            let values = try channels.map { field -> Int in guard let value = Int(field.stringValue), (0...max).contains(value) else { throw NewDocumentOptionsError(L10n.format("背景 RGBA 须为 0–{0} 的整数。", String(describing: max))) }; return value }
            result["background"] = values
        default: break
        }
        return result
    }
}

private struct NewDocumentOptionsError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
