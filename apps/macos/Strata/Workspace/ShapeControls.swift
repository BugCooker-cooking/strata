import SwiftUI

struct ShapeControls: View {
    @ObservedObject var input: CanvasShapeInput
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Picker(L10n.text("形状"), selection: $input.tool) {
                    ForEach(CanvasShapeInput.Tool.allCases, id: \.self) { item in Text(item.title).tag(item) }
                }
                if input.tool == .rounded {
                    LabeledContent(L10n.text("圆角半径")) { TextField("px", value: $input.radius, format: .number) }
                } else if input.tool == .line {
                    LabeledContent(L10n.text("线宽")) { TextField("px", value: $input.weight, format: .number) }
                } else if input.tool == .polygon {
                    Stepper(value: $input.sides, in: 3...128) { Text(L10n.format("边数 {0}", String(input.sides))) }
                }
                coordinates(L10n.text("起点"), $input.start)
                coordinates(L10n.text("终点"), $input.end)
                HStack {
                    Button(L10n.text("新建形状")) { input.apply(create: true) }
                    if input.target != nil {
                        Button(L10n.text("更新所选形状")) { input.apply(create: false) }
                    }
                }
                if input.target != nil {
                    Button(L10n.text("将前景色应用到形状")) { input.useForeground() }
                }
                if !input.error.isEmpty { Text(input.error).foregroundStyle(Color(nsColor: WorkspaceColors.danger)) }
            }.padding(10)
        }.disabled(!input.enabled)
    }
    private func coordinates(_ label: String, _ value: Binding<CGPoint>) -> some View {
        let x = Binding<Double>(get: { Double(value.wrappedValue.x) }, set: { value.wrappedValue.x = CGFloat($0) })
        let y = Binding<Double>(get: { Double(value.wrappedValue.y) }, set: { value.wrappedValue.y = CGFloat($0) })
        return HStack {
            Text(label)
            TextField("X", value: x, format: .number).accessibilityLabel(label + " X")
            TextField("Y", value: y, format: .number).accessibilityLabel(label + " Y")
        }
    }
}
