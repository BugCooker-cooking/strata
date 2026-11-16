import SwiftUI

struct SelectionToolOptions: View {
    @ObservedObject var model: WorkspaceModel
    @State private var showModify = false
    private func update<T>(_ key: ReferenceWritableKeyPath<WorkspaceModel, T>) -> Binding<T> {
        Binding(get: { model[keyPath: key] }, set: { model[keyPath: key] = $0; model.send(.tool(model.tool)) })
    }
    var body: some View {
        HStack(spacing: 9) {
            switch model.tool {
            case .marquee:
                Picker(L10n.text("选框形状"), selection: update(\.marqueeShape)) { Text(L10n.text("矩形")).tag("rectangle"); Text(L10n.text("椭圆")).tag("ellipse") }.frame(width: 86)
            case .lasso:
                Picker(L10n.text("套索类型"), selection: update(\.lassoShape)) { Text(L10n.text("自由套索")).tag("freehand"); Text(L10n.text("多边形")).tag("polygon") }.frame(width: 104)
            default:
                Picker(L10n.text("取样选区"), selection: update(\.selectionShape)) { Text(L10n.text("快速选择")).tag("quick"); Text(L10n.text("魔棒")).tag("wand") }.frame(width: 104)
            }
            Picker(L10n.text("选区运算"), selection: update(\.selectionMode)) {
                Text(L10n.text("新建")).tag("replace"); Text(L10n.text("相加")).tag("add"); Text(L10n.text("相减")).tag("subtract"); Text(L10n.text("交叉")).tag("intersect")
            }.frame(width: 82)
            if model.tool == .selection {
                Picker(L10n.text("取样"), selection: update(\.selectionSample)) { Text(L10n.text("可见图层")).tag("merged"); Text(L10n.text("当前像素层")).tag("current_layer") }.frame(width: 118)
                if model.selectionShape == "wand" {
                    TextField(L10n.text("容差"), value: update(\.wandTolerance), format: .number.precision(.fractionLength(0))).frame(width: 42).accessibilityLabel(L10n.text("魔棒容差 0 至 255"))
                    Toggle(L10n.text("连续"), isOn: update(\.wandContiguous))
                } else {
                    TextField(L10n.text("半径"), value: update(\.quickRadius), format: .number.precision(.fractionLength(1))).frame(width: 42).accessibilityLabel(L10n.text("快速选择画笔半径"))
                    Toggle(L10n.text("标背景"), isOn: update(\.quickBackground))
                    Text(L10n.text("回车计算 · Esc 取消")).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                }
            } else {
                Text(model.tool == .lasso && model.lassoShape == "polygon" ? L10n.text("双击或回车闭合 · Esc 取消") : L10n.text("⇧ 相加 · ⌥ 相减 · Esc 取消")).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            }
            Menu(L10n.text("选区")) {
                Button(L10n.text("全选")) { model.send(.command("select.all", [:])) }
                Button(L10n.text("取消选择")) { model.send(.command("select.none", [:])) }
                Button(L10n.text("重新选择")) { model.send(.command("select.reselect", [:])) }
                Button(L10n.text("反选")) { model.send(.command("select.inverse", [:])) }
            }.fixedSize()
            Button(L10n.text("修改 / 变换…")) { showModify = true }
                .popover(isPresented: $showModify) { SelectionModificationControls(model: model) }
        }.labelsHidden().disabled(!model.permissions.selectionEdits)
    }
}

struct SelectionModificationControls: View {
    @ObservedObject var model: WorkspaceModel
    @State private var operation = "select.feather"
    @State private var radius = 2.0
    @State private var dx = 0.0
    @State private var dy = 0.0
    @State private var scaleX = 100.0
    @State private var scaleY = 100.0
    @State private var angle = 0.0
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(L10n.text("修改选区")).fontWeight(.semibold)
            Picker(L10n.text("操作"), selection: $operation) {
                Text(L10n.text("羽化")).tag("select.feather"); Text(L10n.text("扩展")).tag("select.expand"); Text(L10n.text("收缩")).tag("select.contract")
                Text(L10n.text("平滑")).tag("select.smooth"); Text(L10n.text("边界")).tag("select.border")
            }
            HStack { Text(L10n.text("半径 / 宽度")); field(L10n.text("像素"), $radius); Text("px") }
            Button(L10n.text("应用修改")) { model.send(.command(operation, ["radius": radius])) }.disabled(!(0...256).contains(radius))
            Divider()
            Text(L10n.text("变换选区")).fontWeight(.semibold)
            HStack { Text(L10n.text("移动")); field("X", $dx); field("Y", $dy); Text("px") }
            HStack { Text(L10n.text("缩放")); field(L10n.text("水平"), $scaleX); field(L10n.text("垂直"), $scaleY); Text("%") }
            HStack { Text(L10n.text("旋转")); field(L10n.text("角度"), $angle); Text("°") }
            Text(L10n.text("以当前选区中心为基点；负缩放可翻转。只变换选区覆盖。")).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            Button(L10n.text("应用变换")) { transform() }.disabled(scaleX == 0 || scaleY == 0)
        }.textFieldStyle(.roundedBorder).padding(16).frame(width: 320)
            .disabled(!model.permissions.selectionEdits || model.snapshot?.selection == nil)
    }
    private func field(_ label: String, _ value: Binding<Double>) -> some View {
        TextField(label, value: value, format: .number.precision(.fractionLength(2))).frame(minWidth: 48).accessibilityLabel(label)
    }
    private func transform() {
        let extent = model.snapshot?.selectionExtent
        let cx = (extent?["x"].number ?? 0) + (extent?["width"].number ?? 0)/2
        let cy = (extent?["y"].number ?? 0) + (extent?["height"].number ?? 0)/2
        let radians = angle * .pi / 180, sx = scaleX/100, sy = scaleY/100
        let a = cos(radians)*sx, b = sin(radians)*sx, c = -sin(radians)*sy, d = cos(radians)*sy
        model.send(.command("select.transform", ["matrix": [a,b,c,d,cx+dx-a*cx-c*cy,cy+dy-b*cx-d*cy]]))
    }
}
