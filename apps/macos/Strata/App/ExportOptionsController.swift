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
    let originalSize=NSButton(checkboxWithTitle:"每份文档使用原尺寸",target:nil,action:nil)
    let view=NSStackView()
    private let presetEditing:Bool
    private let sourceWidth: UInt32
    private let sourceHeight: UInt32
    let quantization = NSButton(checkboxWithTitle: "允许此副本从 16 位量化为 8 位", target: nil, action: nil)
    private let warning = NSTextField(wrappingLabelWithString: "")
    private let bitDepth: UInt8
    private let kinds = ["png", "tiff", "jpeg", "webp"]
    private let extensions = ["png", "tiff", "jpg", "webp"]
    private let types: [UTType] = [.png, .tiff, .jpeg, .webP]

    init(panel: NSSavePanel? = nil, bitDepth: UInt8, width: UInt32 = 1, height: UInt32 = 1, presetEditing:Bool=false) {
        self.panel = panel; self.bitDepth = bitDepth;self.presetEditing=presetEditing
        sourceWidth = width; sourceHeight = height
        super.init()
        self.width.stringValue = String(width); self.height.stringValue = String(height)
        self.width.delegate = self; self.height.delegate = self; lockAspect.state = .on
        originalSize.state = .on;originalSize.isHidden = !presetEditing;originalSize.target=self;originalSize.action=#selector(changeOriginalSize)
        if presetEditing{quantization.title="允许16位文档的导出副本量化为8位"}
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
        NSLayoutConstraint.activate([self.width.widthAnchor.constraint(equalToConstant:92),self.height.widthAnchor.constraint(equalToConstant:92)])
        let stack = view
        for child in [row("格式", format),originalSize,row("输出尺寸", size),lockAspect,row("输出色域", space),row("WebP 编码", webpMode),row("有损质量", quality),row("透明区域", background),quantization,warning]{stack.addArrangedSubview(child)}
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 12, right: 16)
        stack.frame = NSRect(x: 0, y: 0, width: 380, height: presetEditing ? 350:320)
        stack.widthAnchor.constraint(equalToConstant: 380).isActive = true
        warning.widthAnchor.constraint(equalToConstant: 348).isActive = true
        panel?.accessoryView = stack; panel?.delegate = self
        format.target = self; format.action = #selector(changeFormat(_:)); update()
        lockAspect.target = self; lockAspect.action = #selector(changeAspect)
        webpMode.target = self; webpMode.action = #selector(changeEncoding)
        changeOriginalSize()
    }
    @objc func changeFormat(_ sender: NSPopUpButton) { update() }
    @objc func changeEncoding() { update() }
    @objc func changeOriginalSize(){let enabled = !presetEditing || originalSize.state != .on;width.isEnabled=enabled;height.isEnabled=enabled;lockAspect.isEnabled=enabled}
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
        quantization.isEnabled = (bitDepth == 16 || presetEditing) && index >= 2
        if !quantization.isEnabled { quantization.state = .off }
        updateWarning()
    }
    private func updateWarning() {
        let index = format.indexOfSelectedItem
        warning.stringValue = index == 2
            ? "JPEG 不保留透明度；按所选背景合成，质量越高文件通常越大。文档原件不变。"
            : index == 3 ? (webpMode.indexOfSelectedItem == 0 ? "WebP 无损编码，保留透明度，仅支持 8 位输出。" : "WebP 有损压缩 RGB，透明度保持无损，仅支持 8 位输出。") + "文档原件不变。"
            : (presetEditing ? "每份文档按自身位深保留透明度，并嵌入所选ICC。":"输出保持 \(bitDepth) 位和透明度，并嵌入所选 ICC。")+"调整尺寸会重采样，原文档不变。"
    }
    func parameters(requireQuantization:Bool=true) throws -> [String: Any] {
        let index = format.indexOfSelectedItem
        if requireQuantization && bitDepth == 16 && index >= 2 && quantization.state != .on {
            throw ExportOptionsError("此格式只支持 8 位。请勾选允许量化，或使用 PNG/TIFF 保留 16 位。")
        }
        var result: [String: Any] = ["format": kinds[index]]
        let useOriginal=presetEditing && originalSize.state == .on
        guard let w = useOriginal ? sourceWidth:UInt32(width.stringValue), let h = useOriginal ? sourceHeight:UInt32(height.stringValue), w > 0, h > 0,
              useOriginal || UInt64(w) * UInt64(h) <= (512 * 1024 * 1024) / UInt64(presetEditing || bitDepth != 16 ? 4 : 8) else {
            throw ExportOptionsError("宽高须为正整数，输出像素数据不能超过 512 MiB。")
        }
        if presetEditing ? originalSize.state != .on : (w != sourceWidth || h != sourceHeight) { result["width"] = w; result["height"] = h }
        if index == 3 && (!presetEditing || originalSize.state != .on) && (w > 16383 || h > 16383) { throw ExportOptionsError("WebP 宽高不能超过 16383 像素。") }
        if space.indexOfSelectedItem > 0 { result["target_space"] = space.indexOfSelectedItem == 1 ? "srgb" : "display_p3" }
        if quality.isEnabled {
            guard let value = Int(quality.stringValue), (1...100).contains(value) else { throw ExportOptionsError("有损质量须为 1–100 的整数。") }
            result["quality"] = value
        }
        if index == 3 && webpMode.indexOfSelectedItem == 1 { result["webp_lossless"] = false }
        if index == 2 {
            result["background"] = presetMatte ?? Array(repeating: background.indexOfSelectedItem == 0 ? 255 : 0, count: 3)
        }
        if index >= 2 && quantization.state == .on { result["allow_quantization"] = true }
        return result
    }
    func loadPreset(_ options:AIJSON)throws{
        guard let kind=options["format"].string,let index=kinds.firstIndex(of:kind)else{throw ExportOptionsError("预设格式无效。")}
        format.selectItem(at:index);webpMode.selectItem(at:options["webp_lossless"] == .bool(false) ? 1:0)
        space.selectItem(at:options["target_space"].string=="srgb" ? 1:options["target_space"].string=="display_p3" ? 2:0)
        let q=options["quality"].number ?? Double(index==3 ? 80:90)
        guard let qualityValue=UInt8(exactly:q),(1...100).contains(qualityValue)else{throw ExportOptionsError("预设质量须为1–100的整数。")}
        quality.stringValue=String(qualityValue)
        let matte=options["background"].array.compactMap(\.number)
        // Native presets accept arbitrary RGB matte from AI; retain it unless
        // the user explicitly selects one of the two native matte choices.
        if !matte.isEmpty{guard matte.count==3,matte.allSatisfy({UInt8(exactly:$0) != nil})else{throw ExportOptionsError("预设背景须为0–255的三个整数。")}}
        presetMatte=matte.count==3 ? matte.map(Int.init):nil
        while background.numberOfItems>2{background.removeItem(at:2)}
        if let presetMatte,presetMatte != [0,0,0],presetMatte != [255,255,255]{background.addItem(withTitle:"RGB \(presetMatte.map(String.init).joined(separator:", "))");background.selectItem(at:2)}else{background.selectItem(at:matte==[0,0,0] ? 1:0)}
        background.target=self;background.action=#selector(changeMatte)
        originalSize.state=options["width"].number==nil ? .on:.off
        if let w=options["width"].number,let h=options["height"].number{guard let exactW=UInt32(exactly:w),let exactH=UInt32(exactly:h),exactW>0,exactH>0 else{throw ExportOptionsError("预设宽高须为正整数。")};width.stringValue=String(exactW);height.stringValue=String(exactH)}
        quantization.state=options["allow_quantization"].bool ? .on:.off
        update();changeOriginalSize()
    }
    private var presetMatte:[Int]?
    @objc private func changeMatte(){if background.indexOfSelectedItem<2{presetMatte=nil}}
    func panel(_ sender: Any, validate url: URL) throws { _ = try parameters() }
}

private struct ExportOptionsError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
