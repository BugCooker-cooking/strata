import SwiftUI

struct PathControls: View {
    @ObservedObject var model: WorkspaceModel
    @ObservedObject var input: CanvasPathInput
    @State private var threshold = 32768
    @State private var tolerance = 0.0
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Picker(L10n.text("工作路径"), selection: Binding(get: { input.selectedID ?? "" }, set: { input.select($0) })) {
                    Text(L10n.text("新路径")).tag("")
                    ForEach(input.paths) { item in Text(item.name).tag(item.id) }
                }
                TextField(L10n.text("路径名称"), text: $input.name)
                HStack {
                    Picker(L10n.text("子轮廓"), selection: Binding(get: { input.contour }, set: { input.chooseContour($0) })) {
                        ForEach(input.contours.indices, id: \.self) { i in Text(String(i + 1)).tag(i) }
                    }
                    Button { input.addContour() } label: { Image(systemName: "plus") }
                        .accessibilityLabel(L10n.text("添加子轮廓"))
                        .disabled(!input.canAddContour)
                    Button { input.removeContour() } label: { Image(systemName: "minus") }
                        .accessibilityLabel(L10n.text("删除子轮廓")).disabled(input.contours.count == 1)
                }
                Toggle(L10n.text("开放轮廓"), isOn: Binding(get: { input.openContours.contains(input.contour) }, set: { input.setOpen($0) }))
                Text(L10n.text("单击加点，拖出曲线。Shift 点选多个锚点，⌘ 拖动整条路径，Option 拖动锚点生成对称手柄。回车应用，Esc 取消。"))
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button(L10n.text("平分曲线段")) { input.splitNext() }
                        .disabled(!input.hasSavedGeometry || input.index == nil || input.anchors.count >= 1024 || (input.openContours.contains(input.contour) && input.index == input.anchors.count - 1))
                    Button(L10n.text("转为角点")) { input.corner() }.disabled(input.index == nil)
                    Button(L10n.text("删除锚点")) { input.removeAnchor() }
                        .disabled(!input.canRemoveAnchors)
                }
                Button(L10n.text("解除手柄联动")) { input.unlinkHandles() }
                    .disabled(!input.selectedAnchors.contains { input.anchors.indices.contains($0) && input.anchors[$0].linked == true })
                Button(L10n.text("用前景色填充到新图层")) {
                    guard let id = input.selectedID, let rgba = model.snapshot?.paintColors?.foreground else { return }
                    let maxCode = model.snapshot?.bitDepth == 16 ? 65535.0 : 255.0
                    model.send(.command("layer.fill.path", ["path": ["id": id], "name": input.name, "color": rgba.map { Int(($0 * maxCode).rounded()) }]))
                }.disabled(!input.hasSavedGeometry)
                HStack {
                    Button(L10n.text("应用路径")) { input.apply() }.disabled(!input.canApply)
                    Button(L10n.text("路径转选区")) { input.loadSelection() }.disabled(!input.hasSavedGeometry)
                    Button(L10n.text("删除路径")) { input.deletePath() }.disabled(input.selectedID == nil)
                }
                DisclosureGroup(L10n.text("从选区建立路径")) {
                    LabeledContent(L10n.text("覆盖阈值")) {
                        TextField("1–65535", value: $threshold, format: .number).frame(width: 72)
                    }
                    LabeledContent(L10n.text("简化容差（像素）")) {
                        TextField("0–10", value: $tolerance, format: .number).frame(width: 72)
                    }
                    Button(L10n.text("选区转工作路径")) { input.trace(threshold: threshold, tolerance: tolerance) }
                        .disabled(model.snapshot?.selection == nil || !(1...65535).contains(threshold) ||
                                  !(0...10).contains(tolerance) || input.hasDraftChanges)
                }
            }.padding(10)
        }.disabled(!input.enabled || !model.permissions.edits)
    }
}
