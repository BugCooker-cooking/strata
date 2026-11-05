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
                Picker("选框形状", selection: update(\.marqueeShape)) { Text("矩形").tag("rectangle"); Text("椭圆").tag("ellipse") }.frame(width: 86)
            case .lasso:
                Picker("套索类型", selection: update(\.lassoShape)) { Text("自由套索").tag("freehand"); Text("多边形").tag("polygon") }.frame(width: 104)
            default:
                Picker("取样选区", selection: update(\.selectionShape)) { Text("快速选择").tag("quick"); Text("魔棒").tag("wand") }.frame(width: 104)
            }
            Picker("选区运算", selection: update(\.selectionMode)) {
                Text("新建").tag("replace"); Text("相加").tag("add"); Text("相减").tag("subtract"); Text("交叉").tag("intersect")
            }.frame(width: 82)
            if model.tool == .selection {
                Picker("取样", selection: update(\.selectionSample)) { Text("可见图层").tag("merged"); Text("当前像素层").tag("current_layer") }.frame(width: 118)
                if model.selectionShape == "wand" {
                    TextField("容差", value: update(\.wandTolerance), format: .number.precision(.fractionLength(0))).frame(width: 42).accessibilityLabel("魔棒容差 0 至 255")
                    Toggle("连续", isOn: update(\.wandContiguous))
                } else {
                    TextField("半径", value: update(\.quickRadius), format: .number.precision(.fractionLength(1))).frame(width: 42).accessibilityLabel("快速选择画笔半径")
                    Toggle("标背景", isOn: update(\.quickBackground))
                    Text("回车计算 · Esc 取消").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                }
            } else {
                Text(model.tool == .lasso && model.lassoShape == "polygon" ? "双击或回车闭合 · Esc 取消" : "⇧ 相加 · ⌥ 相减 · Esc 取消").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            }
            Menu("选区") {
                Button("全选") { model.send(.command("select.all", [:])) }
                Button("取消选择") { model.send(.command("select.none", [:])) }
                Button("重新选择") { model.send(.command("select.reselect", [:])) }
                Button("反选") { model.send(.command("select.inverse", [:])) }
            }.fixedSize()
            Button("修改 / 变换…") { showModify = true }
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
            Text("修改选区").fontWeight(.semibold)
            Picker("操作", selection: $operation) {
                Text("羽化").tag("select.feather"); Text("扩展").tag("select.expand"); Text("收缩").tag("select.contract")
                Text("平滑").tag("select.smooth"); Text("边界").tag("select.border")
            }
            HStack { Text("半径 / 宽度"); field("像素", $radius); Text("px") }
            Button("应用修改") { model.send(.command(operation, ["radius": radius])) }.disabled(!(0...256).contains(radius))
            Divider()
            Text("变换选区").fontWeight(.semibold)
            HStack { Text("移动"); field("X", $dx); field("Y", $dy); Text("px") }
            HStack { Text("缩放"); field("水平", $scaleX); field("垂直", $scaleY); Text("%") }
            HStack { Text("旋转"); field("角度", $angle); Text("°") }
            Text("以当前选区中心为基点；负缩放可翻转。只变换选区覆盖。").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            Button("应用变换") { transform() }.disabled(scaleX == 0 || scaleY == 0)
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
