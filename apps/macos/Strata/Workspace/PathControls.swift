import SwiftUI

struct PathControls: View {
    @ObservedObject var model: WorkspaceModel
    @ObservedObject var input: CanvasPathInput
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Picker(L10n.text("工作路径"), selection: Binding(get: { input.selectedID ?? "" }, set: { input.select($0) })) {
                    Text(L10n.text("新路径")).tag("")
                    ForEach(input.paths) { item in Text(item.name).tag(item.id) }
                }
                TextField(L10n.text("路径名称"), text: $input.name)
                Text(L10n.text("单击加点，拖出曲线。Shift 点选多个锚点，⌘ 拖动整条路径，Option 拖动锚点生成对称手柄。回车应用，Esc 取消。"))
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button(L10n.text("平分曲线段")) { input.splitNext() }
                        .disabled(!input.hasSavedGeometry || input.index == nil || input.anchors.count >= 128)
                    Button(L10n.text("转为角点")) { input.corner() }.disabled(input.index == nil)
                    Button(L10n.text("删除锚点")) { input.removeAnchor() }
                        .disabled(!input.canRemoveAnchors)
                }
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
            }.padding(10)
        }.disabled(!input.enabled || !model.permissions.edits)
    }
}
