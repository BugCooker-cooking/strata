import AppKit
import SwiftUI

enum WorkspaceTheme: String, CaseIterable {
    case dark, light, system
    var title: String { switch self { case .dark: "深色"; case .light: "浅色"; case .system: "跟随系统" } }
    var appearance: NSAppearance? {
        switch self { case .dark: NSAppearance(named: .darkAqua); case .light: NSAppearance(named: .aqua); case .system: nil }
    }
}

enum WorkspaceColors {
    static func adaptive(_ dark: UInt32, _ light: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in
            let hex = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
            return NSColor(srgbRed: Double((hex >> 16) & 255) / 255, green: Double((hex >> 8) & 255) / 255,
                           blue: Double(hex & 255) / 255, alpha: 1)
        }
    }
    static let window = adaptive(0x1c1c1e, 0xececee)
    static let panel = adaptive(0x222224, 0xf4f4f5)
    static let raised = adaptive(0x2c2c2f, 0xe7e7ea)
    static let input = adaptive(0x18181a, 0xffffff)
    static let canvas = adaptive(0x141415, 0xd6d6d8)
    static let text = adaptive(0xececee, 0x1d1d1f)
    static let secondary = adaptive(0xa1a1a6, 0x6e6e73)
    static let muted = adaptive(0x6c6c71, 0x808085)
    static let border = adaptive(0x363639, 0xd7d7da)
    static let accent = adaptive(0x0a84ff, 0x007aff)
    static let ai = adaptive(0xa78bfa, 0x7c3aed)
    static let warning = adaptive(0xffb340, 0xc77700)
    static let danger = adaptive(0xff453a, 0xd70015)
}

enum EditorTool: String, CaseIterable, Identifiable {
    case move, marquee, lasso, selection, subject, crop, eyedropper, heal, clone, brush, eraser, bucket, gradient, dodge, text, pen, shape, hand, zoom
    var id: String { rawValue }
    var title: String { Self.info[self]!.0 }
    var symbol: String { Self.info[self]!.1 }
    var key: String { Self.info[self]!.2 }
    var available: Bool { [.move, .marquee, .lasso, .selection, .subject, .brush, .eraser, .clone, .heal, .dodge, .bucket, .gradient, .eyedropper, .hand, .zoom].contains(self) }
    var isPaint: Bool { isBrush || [.bucket,.gradient].contains(self) }
    var isBrush: Bool { [.brush,.eraser,.clone,.heal,.dodge].contains(self) }
    var isSelection: Bool { [.marquee, .lasso, .selection].contains(self) }
    static let groups: [[EditorTool]] = [[.move], [.marquee, .lasso, .selection, .subject], [.crop, .eyedropper], [.heal, .clone, .brush, .eraser, .bucket, .gradient, .dodge], [.text, .pen, .shape], [.hand, .zoom]]
    private static let info: [EditorTool: (String, String, String)] = [
        .move: ("移动", "cursorarrow", "V"), .marquee: ("矩形选框", "rectangle.dashed", "M"),
        .lasso: ("套索", "lasso", "L"), .selection: ("快速选择", "wand.and.rays", "W"),
        .subject: ("选择主体", "person.crop.rectangle", ""), .crop: ("裁剪", "crop", "C"),
        .eyedropper: ("吸管", "eyedropper", "I"), .heal: ("修复画笔", "bandage", "J"),
        .clone: ("仿制图章", "seal", "S"), .brush: ("画笔", "paintbrush.pointed", "B"),
        .eraser: ("橡皮擦", "eraser", "E"), .bucket: ("油漆桶", "drop.fill", "K"), .gradient: ("渐变", "square.lefthalf.filled", "G"),
        .dodge: ("减淡 / 加深 / 海绵", "sun.max", "O"), .text: ("文字", "textformat", "T"),
        .pen: ("钢笔", "pencil.tip.crop.circle", "P"), .shape: ("形状", "square.on.circle", "U"),
        .hand: ("抓手", "hand.draw", "H"), .zoom: ("缩放", "magnifyingglass", "Z")]
}

enum InspectorTab: String, CaseIterable { case properties = "属性", adjustments = "调整", filters = "滤镜", history = "历史" }

/// One policy shared by panels, menu validation and keyboard actions. Core remains authoritative.
struct WorkspacePermissions {
    let busy: Bool
    let control: String
    var edits: Bool { !busy && control == "user" }
    var history: Bool { !busy && ["user", "paused"].contains(control) }
    var visibility: Bool { history }
    var selection: Bool { !busy && ["user", "running", "paused"].contains(control) }
    var selectionEdits: Bool { !busy && ["user", "paused"].contains(control) }
    var status: String {
        switch control {
        case "running": "AI 执行中 · 编辑已锁定"
        case "paused": "AI 已暂停 · 可撤销与切换显隐"
        case "user": "未接入模型 · 手动模式"
        default: "等待编辑状态"
        }
    }
}

@MainActor
final class WorkspaceModel: ObservableObject {
    @Published var assistant: AssistantModel?
    @Published var subjectModel: SubjectModelStore?
    @Published var tonalPreview: TonalPreviewModel?
    @Published var snapshot: EditorSnapshot?
    @Published var thumbnails: [String: NSImage] = [:]
    @Published var fileName = "未命名"
    @Published var busy = false
    @Published var recoveryStatus = ""
    @Published var recoveryError = ""
    @Published var tool = EditorTool.hand
    @Published var selectionMode = "replace"
    @Published var marqueeShape = "rectangle"
    @Published var lassoShape = "freehand"
    @Published var selectionShape = "quick"
    @Published var selectionSample = "merged"
    @Published var wandTolerance = 32.0
    @Published var wandContiguous = true
    @Published var quickRadius = 8.0
    @Published var quickSmoothness = 8.0
    @Published var quickBackground = false
    @Published var brushTarget = "pixels"
    @Published var brushSize = 24.0
    @Published var brushHardness = 80.0
    @Published var brushOpacity = 100.0
    @Published var brushFlow = 100.0
    @Published var brushSpacing = 20.0
    @Published var brushPressureSize = true
    @Published var brushPressureOpacity = true
    @Published var foregroundColor = NSColor.black
    @Published var backgroundColor = NSColor.white
    @Published var paintSample = "merged"
    @Published var sampleRadius = 0.0
    @Published var sampledColorSummary = ""
    @Published var bucketTolerance = 32.0
    @Published var bucketContiguous = true
    @Published var paintOpacity = 100.0
    @Published var gradientShape = "linear"
    @Published var gradientReverse = false
    @Published var cloneSource:[Double]?
    @Published var cloneAligned = true
    @Published var cloneSample = "current_layer"
    @Published var healMode = "sampled"
    @Published var spotSearchRadius = 64.0
    @Published var tonalBrushMode = "dodge"
    @Published var tonalBrushRange = "midtones"
    @Published var tonalBrushExposure = 50.0
    @Published var tonalBrushProtect = true
    @Published var spongeMode = "desaturate"
    @Published var spongeVibrance = true
    @Published var histogram: [Double] = []
    @Published var workingColorSpace=NSColorSpace.sRGB
    @Published var collapsedGroups:Set<String>=[]
    @Published var selectedLayers:Set<String>=[]
    @Published var inspector = InspectorTab.properties
    @Published var assistantVisible = true
    @Published var inspectorVisible = true
    @Published var theme = WorkspaceTheme(rawValue: UserDefaults.standard.string(forKey: "workspace.theme") ?? "dark") ?? .dark
    var action: ((WorkspaceAction) -> Void)?
    var permissions: WorkspacePermissions { WorkspacePermissions(busy: busy, control: snapshot?.control?.kind ?? "user") }
    var selected: EditorSnapshot.Layer? { snapshot?.layers.first { $0.id == snapshot?.activeLayer } }
    var displayLayers:[EditorSnapshot.Layer]{
        let layers=snapshot?.layers ?? []
        func children(_ parent:String?,depth:Int)->[EditorSnapshot.Layer]{
            guard depth<=16 else{return []}
            return layers.reversed().filter{$0.parent==parent}.flatMap{layer in
                [layer]+(layer.isGroup && !collapsedGroups.contains(layer.id) ? children(layer.id,depth:depth+1) : [])
            }
        }
        return children(nil,depth:0)
    }
    func layerDepth(_ layer:EditorSnapshot.Layer)->Int{
        var parent=layer.parent;var depth=0
        while let id=parent,depth<16{depth+=1;parent=snapshot?.layers.first{$0.id==id}?.parent}
        return depth
    }
    var hasDocument: Bool { snapshot?.frame != nil }
    var selectionSummary: String? {
        guard snapshot?.selection != nil else { return nil }
        guard let extent=snapshot?.selectionExtent else { return "空选区" }
        let x=Int64(extent["x"].number ?? 0),y=Int64(extent["y"].number ?? 0)
        let width=Int64(extent["width"].number ?? 0),height=Int64(extent["height"].number ?? 0)
        return "选区 \(x), \(y) · \(width)×\(height)"
    }
    var canUndo: Bool { permissions.history && (snapshot?.historyCursor ?? 0) > 0 }
    var canRedo: Bool { permissions.history && (snapshot?.historyCursor ?? 0) < (snapshot?.history.count ?? 0) }
    func send(_ action: WorkspaceAction) { self.action?(action) }
    func selectTool(_ tool: EditorTool) {
        guard !busy, tool.available, tool != .move || permissions.edits, !tool.isPaint || permissions.edits, !tool.isSelection || permissions.selectionEdits else { return }
        if tool == .subject { if permissions.edits { send(.selectSubject) }; return }
        self.tool = tool; send(.tool(tool))
    }
    func swapPaintColors() { let old = foregroundColor; foregroundColor = backgroundColor; backgroundColor = old; send(.tool(tool)) }
}

enum WorkspaceAction {
    case alignSelected(String),distributeSelected(String)
    case groupSelected
    case cancelWork
    case selectSubject
    case prepareSubjectModel
    case importSubjectModel, removeSubjectModel
    case command(String, [String: Any])
    case exposureBegin, exposurePreview(Double), exposureEnd(Double, Bool)
    case new, open, place, save, export, undo, redo, fit, zoom(Double), actualSize
    case newLayer, duplicateLayer, deleteLayer, reorder(Int), selectLayer(String), rename(String)
    case visibility(String, Bool), opacity(Double), blend(String), position(Int64, Int64)
    case tool(EditorTool), assistant, panels, theme(WorkspaceTheme), newTab
}

@MainActor
final class CanvasReadout: ObservableObject {
    @Published var zoom: Double = 1
    @Published var coordinates = "—"
}

extension EditorSnapshot.History {
    var title: String { commandTitle(command) }
}

func commandTitle(_ command: String) -> String {
    commandTitles[command] ?? command
}
private let commandTitles: [String: String] = [
     "filter.gaussian_blur":"高斯模糊", "filter.unsharp_mask":"USM 锐化", "filter.add_noise":"添加杂色",
     "layer.create": "新建图层", "layer.duplicate": "复制图层", "layer.delete": "删除图层", "layer.rename": "重命名图层",
     "layer.set_position": "移动图层", "layer.set_visibility": "图层显隐", "layer.set_opacity": "调整不透明度",
     "layer.set_blend_mode": "混合模式", "layer.move_order": "调整顺序", "color.convert": "转换色彩空间",
     "batch": "批量编辑", "document.batch": "批量编辑", "file.save": "保存工程", "file.save_as": "另存工程", "file.export": "导出图片",
     "file.place": "置入图片", "select.subject": "选择主体", "select.marquee": "矩形选区",
     "select.all": "全选", "select.none": "取消选择", "select.inverse": "反选",
     "select.ellipse": "椭圆选区", "select.polygon": "多边形套索", "select.lasso": "自由套索",
     "select.wand": "魔棒选区", "select.quick": "快速选择", "select.reselect": "重新选择",
     "select.feather": "羽化选区", "select.expand": "扩展选区", "select.contract": "收缩选区",
     "select.smooth": "平滑选区", "select.border": "选区边界", "select.transform": "变换选区",
     "layer.add_tonal_adjustment":"新建调色调整层", "adjust.tonal.set":"修改调色参数",
     "paint.stroke":"画笔笔触", "paint.erase":"橡皮擦笔触",
     "paint.fill":"填充像素", "paint.bucket":"油漆桶", "paint.gradient":"渐变像素", "paint.sample_color":"吸取颜色",
     "retouch.clone":"仿制图章笔触",
     "retouch.heal":"修复画笔笔触",
     "retouch.spot_heal":"污点修复笔触",
     "retouch.dodge":"减淡笔触", "retouch.burn":"加深笔触", "retouch.sponge":"海绵笔触",
     "layer.add_adjustment": "新建曝光调整层", "adjust.exposure.set": "调整曝光",
     "layer.mask.add": "添加蒙版", "layer.mask.invert": "反相蒙版",
     "layer.mask.set_enabled": "切换蒙版", "layer.mask.remove": "移除蒙版",
     "layer.set_fill_opacity":"填充不透明度","layer.set_locks":"图层锁定","layer.set_color_label":"颜色标签",
     "layer.group":"编组","layer.create_group":"新建组","layer.ungroup":"取消编组","layer.reparent":"移动到组",
     "layer.fill.solid":"纯色填充","layer.fill.gradient":"渐变填充","layer.add_hue_saturation":"色相/饱和度调整",
     "layer.rasterize":"栅格化","layer.mask.apply":"应用蒙版","layer.merge_down":"向下合并",
     "layer.merge_visible":"合并可见","layer.flatten":"拼合图像","layer.stamp_visible":"盖印可见","layer.set_clipping":"剪贴蒙版",
     "layer.set_pass_through":"组穿透模式", "layer.fill.solid.set":"修改纯色填充", "layer.fill.gradient.set":"修改渐变填充",
     "adjust.hue_saturation.set":"调整色相/饱和度", "layer.set_styles":"修改图层样式",
     "layer.align":"对齐图层", "layer.distribute":"分布图层", "layer.via_copy":"通过拷贝新建图层",
     "layer.via_cut":"通过剪切新建图层", "layer.background_to_layer":"背景转普通图层",
     "edit.copy":"拷贝像素", "edit.copy_merged":"合并拷贝", "edit.cut":"剪切像素",
     "edit.paste":"粘贴像素", "edit.paste_in_place":"原位粘贴", "edit.clear":"清除像素"
]
