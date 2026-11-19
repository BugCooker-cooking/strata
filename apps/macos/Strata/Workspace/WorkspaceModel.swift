import AppKit
import SwiftUI

enum WorkspaceTheme: String, CaseIterable {
    case dark, light, system
    var title: String { switch self { case .dark: L10n.text("深色"); case .light: L10n.text("浅色"); case .system: L10n.text("跟随系统") } }
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
    case move, freeTransform, marquee, lasso, selection, subject, crop, eyedropper, heal, clone, brush, eraser, bucket, gradient, dodge, text, pen, shape, hand, zoom
    var id: String { rawValue }
    var title: String { Self.info[self]!.0 }
    var symbol: String { Self.info[self]!.1 }
    var key: String { Self.info[self]!.2 }
    var available: Bool { [.pen, .move, .freeTransform, .text, .marquee, .lasso, .selection, .subject, .crop, .brush, .eraser, .clone, .heal, .dodge, .bucket, .gradient, .eyedropper, .hand, .zoom].contains(self) }
    var isPaint: Bool { isBrush || [.bucket,.gradient].contains(self) }
    var isBrush: Bool { [.brush,.eraser,.clone,.heal,.dodge].contains(self) }
    var isSelection: Bool { [.marquee, .lasso, .selection].contains(self) }
    static let groups: [[EditorTool]] = [[.move,.freeTransform], [.marquee, .lasso, .selection, .subject], [.crop, .eyedropper], [.heal, .clone, .brush, .eraser, .bucket, .gradient, .dodge], [.text, .pen, .shape], [.hand, .zoom]]
    private static let info: [EditorTool: (String, String, String)] = [
        .move: (L10n.text("移动"), "cursorarrow", "V"), .marquee: (L10n.text("矩形选框"), "rectangle.dashed", "M"),
        .freeTransform: (L10n.text("自由变换"), "arrow.up.left.and.arrow.down.right", ""),
        .lasso: (L10n.text("套索"), "lasso", "L"), .selection: (L10n.text("快速选择"), "wand.and.rays", "W"),
        .subject: (L10n.text("选择主体"), "person.crop.rectangle", ""), .crop: (L10n.text("裁剪"), "crop", "C"),
        .eyedropper: (L10n.text("吸管"), "eyedropper", "I"), .heal: (L10n.text("修复画笔"), "bandage", "J"),
        .clone: (L10n.text("仿制图章"), "seal", "S"), .brush: (L10n.text("画笔"), "paintbrush.pointed", "B"),
        .eraser: (L10n.text("橡皮擦"), "eraser", "E"), .bucket: (L10n.text("油漆桶"), "drop.fill", "K"), .gradient: (L10n.text("渐变"), "square.lefthalf.filled", "G"),
        .dodge: (L10n.text("减淡 / 加深 / 海绵"), "sun.max", "O"), .text: (L10n.text("文字"), "textformat", "T"),
        .pen: (L10n.text("钢笔"), "pencil.tip.crop.circle", "P"), .shape: (L10n.text("形状"), "square.on.circle", "U"),
        .hand: (L10n.text("抓手"), "hand.draw", "H"), .zoom: (L10n.text("缩放视图"), "magnifyingglass", "Z")]
}

enum InspectorTab: CaseIterable {
    case properties, adjustments, filters, transform, color, guides, history

    var title: String {
        switch self {
        case .properties: L10n.text("属性")
        case .adjustments: L10n.text("调整")
        case .filters: L10n.text("滤镜")
        case .transform: L10n.text("变换")
        case .color: L10n.text("颜色")
        case .guides: L10n.text("参考线")
        case .history: L10n.text("历史")
        }
    }
}

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
        case "running": L10n.text("AI 执行中 · 编辑已锁定")
        case "paused": L10n.text("AI 已暂停 · 可撤销与切换显隐")
        case "user": L10n.text("未接入模型 · 手动模式")
        default: L10n.text("等待编辑状态")
        }
    }
}

@MainActor
final class WorkspaceModel: ObservableObject {
    @Published var assistant: AssistantModel?
    @Published var subjectModel: SubjectModelStore?
    @Published var tonalPreview: TonalPreviewModel?
    @Published var cropInput:CanvasCropInput?
    @Published var straightenInput:CanvasStraightenInput?
    @Published var transformInput:CanvasTransformInput?
    @Published var textEditing:TextEditingModel?
    @Published var guideInput:CanvasGuideInput?
    @Published var snapInput:CanvasSnapInput?
    @Published var rulerOriginInput:CanvasRulerOriginInput?
    @Published var canvasTransformMode="scale"
    @Published var canvasTransformScope="whole_layer"
    @Published var canvasTransformInterpolation="bilinear"
    @Published var colorPanel:ColorPanelModel?
    @Published var cropMode="crop"
    @Published var cropRatio="free"
    @Published var cropDeletesPixels=false
    @Published var straightenAlign="horizontal"
    @Published var straightenAutoCrop=true
    @Published var straightenInterpolation="bilinear"
    @Published var snapshot: EditorSnapshot?
    @Published var thumbnails: [String: NSImage] = [:]
    @Published var fileName = L10n.text("未命名")
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
    @Published var spotHealMode = "proximity_match"
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
    let pathInput = CanvasPathInput()
    @Published var inspector = InspectorTab.properties
    @Published var assistantVisible = true
    @Published var inspectorVisible = true
    @Published var theme = WorkspaceTheme(rawValue: UserDefaults.standard.string(forKey: "workspace.theme") ?? "dark") ?? .dark
    var action: ((WorkspaceAction) -> Void)?
    var permissions: WorkspacePermissions { WorkspacePermissions(busy: busy, control: snapshot?.control?.kind ?? "user") }
    var selected: EditorSnapshot.Layer? { snapshot?.layers.first { $0.id == snapshot?.activeLayer } }
    var layerSelection: Set<String> {
        if selectedLayers.isEmpty, let current = snapshot?.activeLayer { return [current] }
        return selectedLayers
    }
    var transformLayerIDs: [String] { selectedLayers.count > 1 ? selectedLayers.sorted() : [] }
    func canTransform(scope: String, onCanvas: Bool = false) -> Bool {
        guard permissions.edits else { return false }
        if transformLayerIDs.isEmpty {
            guard let selected, selected.canTransform(scope: scope) else { return false }
            return (!onCanvas || selected.visible) && (scope != "selection" || snapshot?.selectionExtent != nil)
        }
        let layers = snapshot?.layers.filter { selectedLayers.contains($0.id) } ?? []
        return (2...64).contains(layers.count) && layers.contains(where: \.visible)
            && layers.allSatisfy { layer in
                layer.locks?.all != true && layer.locks?.pixels != true
                    && layer.locks?.position != true && layer.locks?.transparency != true
            }
    }
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
        guard let extent=snapshot?.selectionExtent else { return L10n.text("空选区") }
        let x=Int64(extent["x"].number ?? 0),y=Int64(extent["y"].number ?? 0)
        let width=Int64(extent["width"].number ?? 0),height=Int64(extent["height"].number ?? 0)
        return L10n.format("选区 {0}, {1} · {2}×{3}", String(describing: x), String(describing: y), String(describing: width), String(describing: height))
    }
    var canUndo: Bool { permissions.history && (snapshot?.historyCursor ?? 0) > 0 }
    var canRedo: Bool { permissions.history && (snapshot?.historyCursor ?? 0) < (snapshot?.history.count ?? 0) }
    func send(_ action: WorkspaceAction) { self.action?(action) }
    func selectTool(_ tool: EditorTool) {
        guard !busy, tool.available, ![EditorTool.move,.freeTransform,.crop,.text,.pen].contains(tool) || permissions.edits, !tool.isPaint || permissions.edits, !tool.isSelection || permissions.selectionEdits else { return }
        if tool == .subject { if permissions.edits { send(.selectSubject) }; return }
        self.tool = tool; send(.tool(tool))
    }
    func swapPaintColors(){guard permissions.edits else{return};send(.command("paint.colors.swap",[:]))}
    func setPaintColor(_ value:NSColor,background:Bool){
        guard permissions.edits,let c=value.usingColorSpace(workingColorSpace)else{return}
        let values=[c.redComponent,c.greenComponent,c.blueComponent,c.alphaComponent].map{min(1,max(0,Double($0)))}
        setPaintComponents(values,background:background)
    }
    func setPaintComponents(_ values:[Double],background:Bool){guard permissions.edits,values.count==4,values.allSatisfy({$0.isFinite})else{return};send(.command("paint.colors.set",[background ? "background":"foreground":values.map{min(1,max(0,$0))}]))}
    func installPaintColors(_ colors:EditorSnapshot.PaintColors){
        func color(_ v:[Double])->NSColor?{guard v.count==4,v.allSatisfy({$0.isFinite&&(0...1).contains($0)})else{return nil};var components=v.map{CGFloat($0)};return NSColor(colorSpace:workingColorSpace,components:&components,count:4)}
        if let c=color(colors.foreground){foregroundColor=c};if let c=color(colors.background){backgroundColor=c}
    }
}

enum WorkspaceAction {
    case configureText
    case configureCanvasTransform
    case alignSelected(String),distributeSelected(String)
    case groupSelected
    case cancelWork
    case selectSubject
    case prepareSubjectModel
    case importSubjectModel, removeSubjectModel
    case command(String, [String: Any])
    case exposureBegin, exposurePreview(Double), exposureEnd(Double, Bool)
    case new, open, place, save, export, undo, redo, fit, zoom(Double), actualSize
    case newLayer, duplicateLayer, deleteLayer, reorder(Int), selectLayer(String), toggleLayerSelection(String), rename(String)
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
     "type.create":L10n.text("新建可编辑文字"),"type.update":L10n.text("编辑文字"),
     "filter.gaussian_blur":L10n.text("高斯模糊"), "filter.unsharp_mask":L10n.text("USM 锐化"), "filter.high_pass":L10n.text("高反差保留"), "filter.add_noise":L10n.text("添加杂色"), "image.auto":L10n.text("自动色调 / 对比度 / 颜色"), "transform.apply":L10n.text("图层自由变换"), "transform.apply_layers":L10n.text("多图层自由变换"), "image.canvas_size":L10n.text("画布大小"), "image.crop":L10n.text("矩形裁剪"), "image.reveal_all":L10n.text("显示全部可见图层内容"), "image.orient":L10n.text("图像旋转与翻转"), "image.trim":L10n.text("裁去透明边"), "image.rotate":L10n.text("整图任意角度旋转"), "image.resize":L10n.text("图像大小与重采样"), "image.straighten":L10n.text("拉直图像与自动裁剪"), "image.convert_bit_depth":L10n.text("转换文档位深"),
     "layer.create": L10n.text("新建图层"), "layer.duplicate": L10n.text("复制图层"), "layer.delete": L10n.text("删除图层"), "layer.rename": L10n.text("重命名图层"),
     "path.create": L10n.text("创建工作路径"), "path.update": L10n.text("编辑路径锚点"), "path.split": L10n.text("平分曲线段"), "path.delete": L10n.text("删除路径"),
     "select.from_path": L10n.text("路径转选区"), "layer.fill.path": L10n.text("路径填充"),
     "layer.set_position": L10n.text("移动图层"), "layer.snap_position":L10n.text("吸附移动图层"), "view.guides.add":L10n.text("新建参考线"), "view.guides.update":L10n.text("修改参考线"), "view.guides.delete":L10n.text("删除参考线"), "view.guides.clear":L10n.text("清除参考线"), "layer.set_visibility": L10n.text("图层显隐"), "layer.set_opacity": L10n.text("调整不透明度"),
     "layer.set_blend_mode": L10n.text("混合模式"), "layer.move_order": L10n.text("调整顺序"), "color.convert": L10n.text("转换色彩空间"), "color.assign":L10n.text("指定色彩配置"),
     "batch": L10n.text("批量编辑"), "document.batch": L10n.text("批量编辑"), "file.save": L10n.text("保存工程"), "file.save_as": L10n.text("另存工程"), "file.revert":L10n.text("恢复到上次保存"), "file.revert_status":L10n.text("读取恢复原文件状态"), "file.export": L10n.text("导出图片"), "file.quick_export":L10n.text("快速导出"), "file.export_preset.get":L10n.text("读取快速导出预设"), "file.export_preset.set":L10n.text("设置快速导出预设"),
     "file.place": L10n.text("置入图片"), "select.subject": L10n.text("选择主体"), "select.marquee": L10n.text("矩形选区"),
     "select.all": L10n.text("全选"), "select.none": L10n.text("取消选择"), "select.inverse": L10n.text("反选"),
     "select.ellipse": L10n.text("椭圆选区"), "select.polygon": L10n.text("多边形套索"), "select.lasso": L10n.text("自由套索"),
     "select.wand": L10n.text("魔棒选区"), "select.quick": L10n.text("快速选择"), "select.reselect": L10n.text("重新选择"),
     "select.feather": L10n.text("羽化选区"), "select.expand": L10n.text("扩展选区"), "select.contract": L10n.text("收缩选区"),
     "select.smooth": L10n.text("平滑选区"), "select.border": L10n.text("选区边界"), "select.transform": L10n.text("变换选区"),
     "layer.add_tonal_adjustment":L10n.text("新建调色调整层"), "adjust.tonal.set":L10n.text("修改调色参数"),
     "paint.stroke":L10n.text("画笔笔触"), "paint.erase":L10n.text("橡皮擦笔触"),
     "paint.fill":L10n.text("填充像素"), "paint.bucket":L10n.text("油漆桶"), "paint.gradient":L10n.text("渐变像素"), "paint.sample_color":L10n.text("吸取颜色"),
     "retouch.clone":L10n.text("仿制图章笔触"),
     "retouch.heal":L10n.text("修复画笔笔触"),
     "retouch.spot_heal":L10n.text("污点修复笔触"),
     "retouch.dodge":L10n.text("减淡笔触"), "retouch.burn":L10n.text("加深笔触"), "retouch.sponge":L10n.text("海绵笔触"),
     "layer.add_adjustment": L10n.text("新建曝光调整层"), "adjust.exposure.set": L10n.text("调整曝光"),
     "layer.mask.add": L10n.text("添加蒙版"), "layer.mask.invert": L10n.text("反相蒙版"),
     "layer.mask.set_enabled": L10n.text("切换蒙版"), "layer.mask.remove": L10n.text("移除蒙版"),
     "layer.set_fill_opacity":L10n.text("填充不透明度"),"layer.set_locks":L10n.text("图层锁定"),"layer.set_color_label":L10n.text("颜色标签"),
     "layer.group":L10n.text("编组"),"layer.create_group":L10n.text("新建组"),"layer.ungroup":L10n.text("取消编组"),"layer.reparent":L10n.text("移动到组"),
     "layer.fill.solid":L10n.text("纯色填充"),"layer.fill.gradient":L10n.text("渐变填充"),"layer.add_hue_saturation":L10n.text("色相/饱和度调整"),
     "layer.rasterize":L10n.text("栅格化"),"layer.mask.apply":L10n.text("应用蒙版"),"layer.merge_down":L10n.text("向下合并"),
     "layer.merge_visible":L10n.text("合并可见"),"layer.flatten":L10n.text("拼合图像"),"layer.stamp_visible":L10n.text("盖印可见"),"layer.set_clipping":L10n.text("剪贴蒙版"),
     "layer.set_pass_through":L10n.text("组穿透模式"), "layer.fill.solid.set":L10n.text("修改纯色填充"), "layer.fill.gradient.set":L10n.text("修改渐变填充"),
     "adjust.hue_saturation.set":L10n.text("调整色相/饱和度"), "layer.set_styles":L10n.text("修改图层样式"),
     "layer.align":L10n.text("对齐图层"), "layer.distribute":L10n.text("分布图层"), "layer.via_copy":L10n.text("通过拷贝新建图层"),
     "layer.via_cut":L10n.text("通过剪切新建图层"), "layer.background_to_layer":L10n.text("背景转普通图层"),
     "edit.copy":L10n.text("拷贝像素"), "edit.copy_merged":L10n.text("合并拷贝"), "edit.cut":L10n.text("剪切像素"),
     "edit.paste":L10n.text("粘贴像素"), "edit.paste_in_place":L10n.text("原位粘贴"), "edit.clear":L10n.text("清除像素")
]
