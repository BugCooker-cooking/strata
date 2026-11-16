import SwiftUI

struct SubjectModelCard: View {
    @ObservedObject var store: SubjectModelStore
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            Label(L10n.text("本地主体选择"), systemImage: "person.crop.rectangle").fontWeight(.medium)
            Text(store.status).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            if store.downloading {
                ProgressView(value: store.progress)
                Button(L10n.text("取消下载")) { store.cancelDownload() }
            } else if !store.installed {
                Button(L10n.text("下载主体模型…")) { model.send(.prepareSubjectModel) }
            }
            if !store.error.isEmpty { Text(store.error).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.danger)).textSelection(.enabled) }
            Picker(L10n.text("推理后端"), selection: Binding(get: { store.backend }, set: { store.selectBackend($0) })) {
                Text("CPU").tag("cpu"); Text("Core ML").tag("core_ml")
            }.disabled(store.downloading || !model.permissions.edits)
            HStack {
                Button(L10n.text("导入模型…")) { model.send(.importSubjectModel) }
                if store.installed { Button(L10n.text("移除缓存…")) { model.send(.removeSubjectModel) } }
            }.disabled(store.downloading || !model.permissions.edits)
            if let asset = try? SubjectModelAsset.current() {
                DisclosureGroup(L10n.text("版本与许可")) {
                    VStack(alignment: .leading, spacing: 5) {
                        Text("\(asset.id) · \(asset.version)")
                        Text(asset.license)
                        Text("SHA-256\n\(asset.sha256)").textSelection(.enabled)
                        if let source = URL(string: asset.source) { Link(L10n.text("模型来源"), destination: source) }
                        Text(L10n.text("仅接受此受支持版本。模型与图片均在本机处理。"))
                    }.font(.system(size: 9)).foregroundStyle(Color(nsColor: WorkspaceColors.muted))
                }
            }
        }.padding(10).background(RoundedRectangle(cornerRadius: 7).fill(Color(nsColor: WorkspaceColors.raised)))
    }
}

struct ExposureControls: View {
    @ObservedObject var model: WorkspaceModel
    @State private var stops = 0.0
    @State private var dragging = false
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(L10n.text("曝光（档）")).font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            HStack {
                Slider(value: $stops, in: -5...5) { editing in
                    dragging = editing
                    if editing { model.send(.exposureBegin) } else { model.send(.exposureEnd(stops, false)) }
                }.accessibilityLabel(L10n.text("曝光档数"))
                TextField(L10n.text("曝光"), value: $stops, format: .number.precision(.fractionLength(2))).frame(width: 60).textFieldStyle(.roundedBorder)
                    .onSubmit {
                        guard let layer = model.selected else { return }
                        model.send(.command("adjust.exposure.set", ["layer": ["id": layer.id], "stops": stops]))
                    }
            }
            Text(L10n.text("可随时调整或撤销")).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.muted))
        }.disabled(!model.permissions.edits)
            .onAppear { stops = model.selected?.exposure ?? 0 }
            .onChange(of: stops) { _, value in if dragging { model.send(.exposurePreview(value)) } }
            .onChange(of: model.selected?.exposure) { _, value in if !dragging { stops = value ?? 0 } }
            .onChange(of: model.selected?.id) { _, _ in if dragging { model.send(.exposureEnd(stops, true)); dragging = false }; stops = model.selected?.exposure ?? 0 }
            .onDisappear { if dragging { model.send(.exposureEnd(stops, true)) } }
    }
}

struct LuminanceHistogram: View {
    let values: [Double]
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L10n.text("亮度直方图")).font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            GeometryReader { geometry in
                Path { path in
                    let maximum = max(1, values.max() ?? 1)
                    let step = geometry.size.width / CGFloat(max(1, values.count))
                    for (index, value) in values.enumerated() {
                        let height = CGFloat(value / maximum) * geometry.size.height
                        path.addRect(CGRect(x: CGFloat(index) * step, y: geometry.size.height - height, width: max(1, step), height: height))
                    }
                }.fill(Color(nsColor: WorkspaceColors.secondary))
            }.frame(height: 58).background(Color(nsColor: WorkspaceColors.input))
                .accessibilityLabel(L10n.text("可见合成图的线性亮度直方图"))
        }
    }
}
