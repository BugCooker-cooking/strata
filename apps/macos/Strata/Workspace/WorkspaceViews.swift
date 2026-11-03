import SwiftUI

struct WorkspaceButtonStyle: ButtonStyle {
    var selected = false
    var tint: NSColor = WorkspaceColors.accent
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(Color(nsColor: selected ? tint : WorkspaceColors.secondary))
            .padding(5)
            .background(RoundedRectangle(cornerRadius: 5).fill(Color(nsColor: selected ? tint : WorkspaceColors.text).opacity(selected ? 0.18 : configuration.isPressed ? 0.12 : 0)))
            .opacity(enabled ? 1 : 0.33)
            .contentShape(Rectangle())
    }
}

struct WorkspaceIconButton: View {
    let title: String
    let symbol: String
    var enabled = true
    var selected = false
    var action: () -> Void = {}
    var body: some View {
        Button(action: action) { Image(systemName: symbol).font(.system(size: 13)).frame(width: 18, height: 18) }
            .buttonStyle(WorkspaceButtonStyle(selected: selected)).disabled(!enabled)
            .accessibilityLabel(title).help(enabled ? title : "\(title) · 当前不可用")
    }
}

struct ToolRail: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        VStack(spacing: 4) {
            ScrollView(.vertical, showsIndicators: false) {
                VStack(spacing: 5) {
                    ForEach(Array(EditorTool.groups.enumerated()), id: \.offset) { index, group in
                        if index > 0 { Rectangle().fill(Color(nsColor: WorkspaceColors.border)).frame(height: 1).padding(.horizontal, 10) }
                        ForEach(group) { tool in
                            WorkspaceIconButton(title: "\(tool.title)\(tool.key.isEmpty ? "" : " (\(tool.key))")", symbol: tool.symbol,
                                enabled: tool.available && (tool != .move || model.permissions.edits), selected: model.tool == tool) { model.selectTool(tool) }
                        }
                    }
                }.padding(.top, 8)
            }
            ZStack {
                RoundedRectangle(cornerRadius: 3).fill(.white).frame(width: 18, height: 18).offset(x: 5, y: 5)
                RoundedRectangle(cornerRadius: 3).fill(.black).frame(width: 18, height: 18).overlay(RoundedRectangle(cornerRadius: 3).stroke(.gray)).offset(x: -3, y: -3)
            }.frame(height: 30).opacity(0.45).help("前景色 / 背景色 · 绘画功能尚未提供")
            WorkspaceIconButton(title: "快速蒙版 (Q)", symbol: "rectangle.inset.filled", enabled: false)
        }.frame(width: 46).padding(.bottom, 6).background(Color(nsColor: WorkspaceColors.panel))
    }
}

struct ToolOptionsBar: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        HStack(spacing: 14) {
            Label(model.tool.title, systemImage: model.tool.symbol).fontWeight(.medium).frame(width: 100, alignment: .leading)
            Divider().frame(height: 16)
            switch model.tool {
            case .move:
                Text("当前图层").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                Text(model.selected?.name ?? "未选择").lineLimit(1).frame(maxWidth: 180, alignment: .leading)
                Text("拖动移动 · Esc 取消").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            case .zoom:
                Text("单击放大 · ⌥ 单击缩小").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                Button("100%") { model.send(.actualSize) }.buttonStyle(WorkspaceButtonStyle())
                Button("适合屏幕") { model.send(.fit) }.buttonStyle(WorkspaceButtonStyle())
            default:
                Text("拖动画布 · 滚动平移 · 捏合缩放").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            }
            Spacer()
            if model.busy { ProgressView().controlSize(.small); Text("正在处理…") }
            Menu {
                ForEach(WorkspaceTheme.allCases, id: \.self) { theme in
                    Button { model.send(.theme(theme)) } label: {
                        if model.theme == theme { Label(theme.title, systemImage: "checkmark") } else { Text(theme.title) }
                    }
                }
            } label: { Image(systemName: "circle.lefthalf.filled") }
            .menuStyle(.borderlessButton).fixedSize().help("界面外观")
        }.font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.text))
            .padding(.horizontal, 12).frame(height: 36).background(Color(nsColor: WorkspaceColors.panel))
    }
}

struct DocumentTitleView: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "photo").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            Text(model.fileName).lineLimit(1).truncationMode(.middle)
            if model.snapshot?.dirty == true { Circle().fill(Color(nsColor: WorkspaceColors.secondary)).frame(width: 6, height: 6).accessibilityLabel("未保存") }
        }.font(.system(size: 12)).padding(.horizontal, 10).padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 5).fill(Color(nsColor: WorkspaceColors.raised)))
    }
}

struct WorkspaceInspector: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        GeometryReader { geometry in
            VStack(spacing: 0) {
                VStack(spacing: 0) {
                    Picker("检查器", selection: $model.inspector) {
                        ForEach(InspectorTab.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                    }.pickerStyle(.segmented).labelsHidden().padding(10)
                    switch model.inspector {
                    case .properties: LayerProperties(model: model)
                    case .adjustments: AdjustmentPanel()
                    case .history: HistoryPanel(model: model)
                    }
                }.frame(height: max(210, geometry.size.height * 0.47))
                Divider()
                LayerPanel(model: model)
            }
        }.font(.system(size: 12)).foregroundStyle(Color(nsColor: WorkspaceColors.text))
            .background(Color(nsColor: WorkspaceColors.panel))
    }
}

private struct LayerProperties: View {
    @ObservedObject var model: WorkspaceModel
    @State private var name = ""
    @State private var x = "0"
    @State private var y = "0"
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Label(model.selected == nil ? "文档" : "像素图层", systemImage: "square.3.layers.3d").fontWeight(.medium)
                if let layer = model.selected {
                    TextField("图层名称", text: $name).textFieldStyle(.roundedBorder).onSubmit { model.send(.rename(name)) }
                        .disabled(!model.permissions.edits).accessibilityLabel("图层名称")
                    caption("位置")
                    HStack {
                        coordinate("X", text: $x); coordinate("Y", text: $y)
                    }
                    caption("外观")
                    BlendOpacityControls(model: model)
                    Toggle("显示图层", isOn: Binding(get: { layer.visible }, set: { model.send(.visibility(layer.id, $0)) }))
                        .disabled(!model.permissions.visibility)
                } else { Text("选择一个图层，查看并修改属性。").foregroundStyle(Color(nsColor: WorkspaceColors.secondary)) }
                if let frame = model.snapshot?.frame {
                    Divider(); caption("画板")
                    HStack { Text(frame.name).lineLimit(1); Spacer(); Text("\(frame.width) × \(frame.height) px").monospacedDigit() }
                    Text("\(model.snapshot?.profileName ?? "RGB") · \(model.snapshot?.bitDepth ?? 8) 位").font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                }
                Spacer(minLength: 0)
            }.padding(12)
        }.onChange(of: model.snapshot?.revision) { _, _ in sync() }
            .onChange(of: model.selected?.id) { _, _ in sync() }.onAppear(perform: sync)
    }
    private func caption(_ text: String) -> some View { Text(text).font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
    private func coordinate(_ title: String, text: Binding<String>) -> some View {
        HStack(spacing: 6) { Text(title).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)); TextField(title, text: text).multilineTextAlignment(.trailing).onSubmit(commitPosition); Text("px").foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
            .padding(6).background(RoundedRectangle(cornerRadius: 4).fill(Color(nsColor: WorkspaceColors.input))).disabled(!model.permissions.edits)
    }
    private func sync() { name = model.selected?.name ?? ""; x = String(model.selected?.x ?? 0); y = String(model.selected?.y ?? 0) }
    private func commitPosition() {
        guard let px = Int64(x), let py = Int64(y) else { sync(); return }
        model.send(.position(px, py))
    }
}

private struct BlendOpacityControls: View {
    @ObservedObject var model: WorkspaceModel
    @State private var opacity = 100.0
    var body: some View {
        VStack(spacing: 8) {
            HStack {
                Text("混合模式").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                Spacer()
                Picker("混合模式", selection: Binding(get: { model.selected?.blendMode ?? "normal" }, set: { model.send(.blend($0)) })) {
                    Text("正常").tag("normal"); Text("正片叠底").tag("multiply"); Text("滤色").tag("screen"); Text("叠加").tag("overlay")
                }.labelsHidden().frame(width: 108)
            }
            HStack(spacing: 8) {
                Text("不透明度").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                Slider(value: $opacity, in: 0...100, onEditingChanged: { editing in if !editing { model.send(.opacity(opacity / 100)) } }).accessibilityLabel("图层不透明度")
                Text("\(Int(opacity.rounded()))%").monospacedDigit().frame(width: 35, alignment: .trailing)
            }
        }.disabled(!model.permissions.edits || model.selected == nil)
            .onAppear { opacity = (model.selected?.opacity ?? 1) * 100 }
            .onChange(of: model.selected?.id) { _, _ in opacity = (model.selected?.opacity ?? 1) * 100 }
            .onChange(of: model.selected?.opacity) { _, value in opacity = (value ?? 1) * 100 }
    }
}

private struct AdjustmentPanel: View {
    private let entries = [("曝光", "plusminus"), ("亮度 / 对比度", "sun.max"), ("色阶", "chart.bar"), ("曲线", "point.topleft.down.to.point.bottomright.curvepath"), ("色相 / 饱和度", "slider.horizontal.3"), ("色彩平衡", "circle.lefthalf.filled"), ("黑白", "circle.righthalf.filled"), ("自然饱和度", "drop"), ("渐变映射", "rectangle.lefthalf.filled"), ("反相", "circle.dotted")]
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("调整图层").fontWeight(.medium)
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())], spacing: 14) {
                    ForEach(entries, id: \.0) { item in
                        VStack(spacing: 7) { WorkspaceIconButton(title: item.0, symbol: item.1, enabled: false); Text(item.0).font(.system(size: 10)).lineLimit(2).multilineTextAlignment(.center) }
                    }
                }
                Text("调整图层将在后续版本提供。当前可在图层属性中修改位置、不透明度和混合模式。")
                    .font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).fixedSize(horizontal: false, vertical: true)
            }.padding(12)
        }
    }
}

private struct HistoryPanel: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 1) {
                if model.snapshot?.history.isEmpty != false {
                    Label("暂无编辑记录", systemImage: "clock.arrow.circlepath").foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).padding(16)
                }
                ForEach(Array((model.snapshot?.history ?? []).enumerated()), id: \.offset) { index, item in
                    HStack(spacing: 9) {
                        Image(systemName: index < (model.snapshot?.historyCursor ?? 0) ? "checkmark.circle" : "arrow.uturn.forward.circle")
                        VStack(alignment: .leading, spacing: 3) { Text(item.title); Text(item.source == "ai" ? "AI" : "你").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)) }
                        Spacer()
                    }.padding(.horizontal, 12).padding(.vertical, 8)
                        .foregroundStyle(Color(nsColor: item.source == "ai" ? WorkspaceColors.ai : WorkspaceColors.text))
                        .opacity(index < (model.snapshot?.historyCursor ?? 0) ? 1 : 0.4)
                        .background(index == (model.snapshot?.historyCursor ?? 0) - 1 ? Color(nsColor: WorkspaceColors.accent).opacity(0.12) : .clear)
                }
                if model.snapshot?.executions.isEmpty == false {
                    Divider().padding(.vertical, 8)
                    Text("文件记录 · 不随文档撤销恢复").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).padding(.horizontal, 12)
                    ForEach(Array((model.snapshot?.executions ?? []).enumerated()), id: \.offset) { _, item in
                        Label("\(commandTitle(item.command)) · \(item.status)", systemImage: "doc.badge.arrow.up").font(.system(size: 11)).padding(10)
                    }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct LayerPanel: View {
    @ObservedObject var model: WorkspaceModel
    @State private var filter = ""
    var layers: [EditorSnapshot.Layer] { Array((model.snapshot?.layers ?? []).reversed()).filter { filter.isEmpty || $0.name.localizedCaseInsensitiveContains(filter) } }
    var body: some View {
        VStack(spacing: 0) {
            HStack { Label("图层", systemImage: "square.3.layers.3d"); Spacer(); Text("\(model.snapshot?.layers.count ?? 0)").foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }.padding(12)
            TextField("查找图层", text: $filter).textFieldStyle(.roundedBorder).padding(.horizontal, 10).padding(.bottom, 10)
            BlendOpacityControls(model: model).padding(.horizontal, 12).padding(.bottom, 10)
            Divider()
            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(layers, id: \.id) { layer in
                        HStack(spacing: 7) {
                            WorkspaceIconButton(title: layer.visible ? "隐藏 \(layer.name)" : "显示 \(layer.name)", symbol: layer.visible ? "eye" : "eye.slash", enabled: model.permissions.visibility) { model.send(.visibility(layer.id, !layer.visible)) }
                            Button { model.send(.selectLayer(layer.id)) } label: {
                                HStack(spacing: 9) {
                                    ZStack {
                                        if let image = model.thumbnails[layer.id] { Image(nsImage: image).resizable().scaledToFit() }
                                        else { Image(systemName: "photo").foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
                                    }.frame(width: 30, height: 30).background(Color(nsColor: WorkspaceColors.raised)).clipShape(RoundedRectangle(cornerRadius: 4))
                                    VStack(alignment: .leading, spacing: 3) { Text(layer.name).lineLimit(1); Text("像素图层").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
                                    Spacer(minLength: 0)
                                }.contentShape(Rectangle())
                            }.buttonStyle(.plain).disabled(!model.permissions.selection)
                        }.padding(.horizontal, 4).padding(.vertical, 4)
                            .background(RoundedRectangle(cornerRadius: 5).fill(model.snapshot?.activeLayer == layer.id ? Color(nsColor: WorkspaceColors.accent).opacity(0.20) : .clear))
                            .contextMenu {
                                Button("复制图层") { model.send(.duplicateLayer) }.disabled(model.selected?.id != layer.id || !model.permissions.edits)
                            }
                    }
                    if layers.isEmpty { Text(filter.isEmpty ? "新建或打开图片开始编辑" : "没有匹配的图层").foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).padding(20) }
                }.padding(5)
            }
            Divider()
            HStack(spacing: 2) {
                WorkspaceIconButton(title: "上移", symbol: "arrow.up", enabled: model.permissions.edits && model.selected != nil) { model.send(.reorder(1)) }
                WorkspaceIconButton(title: "下移", symbol: "arrow.down", enabled: model.permissions.edits && model.selected != nil) { model.send(.reorder(-1)) }
                Spacer(minLength: 0)
                WorkspaceIconButton(title: "添加蒙版", symbol: "rectangle.inset.filled", enabled: false)
                WorkspaceIconButton(title: "新建组", symbol: "folder.badge.plus", enabled: false)
                WorkspaceIconButton(title: "复制图层", symbol: "square.on.square", enabled: model.permissions.edits && model.selected != nil) { model.send(.duplicateLayer) }
                WorkspaceIconButton(title: "新建图层", symbol: "plus.square", enabled: model.permissions.edits && model.hasDocument) { model.send(.newLayer) }
                WorkspaceIconButton(title: "置入图片 (⇧⌘P)", symbol: "photo.badge.plus", enabled: model.permissions.edits && model.hasDocument) { model.send(.place) }
                WorkspaceIconButton(title: "删除图层", symbol: "trash", enabled: model.permissions.edits && model.selected != nil) { model.send(.deleteLayer) }
            }.padding(5)
        }
    }
}

struct AssistantPanel: View {
    @ObservedObject var model: WorkspaceModel
    @State private var provider = "Anthropic"
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "sparkles").foregroundStyle(Color(nsColor: WorkspaceColors.ai)); Text("AI 助手").fontWeight(.semibold)
                Spacer(); Text(model.snapshot?.control?.kind == "running" ? "执行中" : model.snapshot?.control?.kind == "paused" ? "已暂停" : "未接入").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.muted))
                WorkspaceIconButton(title: "收起 AI 助手 (⌘J)", symbol: "sidebar.right") { model.send(.assistant) }
            }.padding(.horizontal, 14).frame(height: 44)
            Divider()
            if let control = model.snapshot?.control?.kind, control != "user" {
                VStack(alignment: .leading, spacing: 8) {
                    Label(model.permissions.status, systemImage: control == "running" ? "lock" : "pause.circle")
                    Text("当前操作记录见历史面板；画布浏览保持可用。").font(.system(size: 10))
                }.frame(maxWidth: .infinity, alignment: .leading).padding(14)
                    .foregroundStyle(Color(nsColor: WorkspaceColors.ai))
                HistoryPanel(model: model)
            } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    VStack(spacing: 12) {
                        Image(systemName: "sparkles").font(.system(size: 24)).foregroundStyle(Color(nsColor: WorkspaceColors.ai)).padding(12).background(RoundedRectangle(cornerRadius: 12).fill(Color(nsColor: WorkspaceColors.ai).opacity(0.12)))
                        Text("你的创作，AI 来协助").font(.system(size: 16, weight: .semibold))
                        Text("模型接入将在后续阶段提供。现在可以使用全部已实现的手动编辑功能。")
                            .foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).multilineTextAlignment(.center).lineSpacing(4)
                    }.frame(maxWidth: .infinity).padding(.vertical, 20)
                    ForEach([("Anthropic", "Claude 系列模型", "sparkles"), ("OpenAI 兼容接口", "自定义服务地址与模型", "network"), ("本地模型", "本机运行的模型服务", "desktopcomputer")], id: \.0) { item in
                        Button { provider = item.0 } label: {
                            HStack(spacing: 12) {
                                Image(systemName: item.2).frame(width: 20)
                                VStack(alignment: .leading, spacing: 4) { Text(item.0).fontWeight(.medium); Text(item.1).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)) }
                                Spacer(); Image(systemName: provider == item.0 ? "largecircle.fill.circle" : "circle").foregroundStyle(Color(nsColor: provider == item.0 ? WorkspaceColors.ai : WorkspaceColors.muted))
                            }.padding(12).background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: WorkspaceColors.raised)))
                                .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(nsColor: provider == item.0 ? WorkspaceColors.ai : WorkspaceColors.border)))
                        }.buttonStyle(.plain)
                    }
                    Label("暂未开放连接", systemImage: "lock").frame(maxWidth: .infinity).padding(10).background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: WorkspaceColors.ai).opacity(0.12))).foregroundStyle(Color(nsColor: WorkspaceColors.ai))
                    VStack(alignment: .leading, spacing: 10) {
                        Label("当前不会读取密钥或发送图片", systemImage: "lock.shield")
                        Label("模型接入后，操作沿用同一命令与历史", systemImage: "square.stack.3d.up")
                        Label("可随时收起面板，专注手动编辑", systemImage: "sidebar.right")
                    }.font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                }.padding(16)
            }
            }
            Divider()
            VStack(alignment: .leading, spacing: 9) {
                HStack { Label(model.selected?.name ?? "未选择图层", systemImage: "square.3.layers.3d").lineLimit(1); Spacer(); Text("画布 + 图层") }.font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                Text("接入模型后，在这里描述你想要的效果…").foregroundStyle(Color(nsColor: WorkspaceColors.muted)).frame(maxWidth: .infinity, minHeight: 42, alignment: .topLeading)
                HStack {
                    WorkspaceIconButton(title: "附加参考图", symbol: "paperclip", enabled: false)
                    WorkspaceIconButton(title: "引用图层", symbol: "at", enabled: false)
                    Spacer(); Text("先问我").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                    WorkspaceIconButton(title: "发送", symbol: "arrow.up.circle.fill", enabled: false)
                }
                Text("文档编辑可撤销；已写入的文件不随撤销恢复。")
                    .font(.system(size: 9)).foregroundStyle(Color(nsColor: WorkspaceColors.muted))
            }.padding(12).background(Color(nsColor: WorkspaceColors.input).opacity(0.5))
        }.font(.system(size: 12)).foregroundStyle(Color(nsColor: WorkspaceColors.text)).background(Color(nsColor: WorkspaceColors.panel))
    }
}

struct WorkspaceStatusBar: View {
    @ObservedObject var model: WorkspaceModel
    @ObservedObject var readout: CanvasReadout
    var body: some View {
        HStack(spacing: 14) {
            Text("\(Int((readout.zoom * 100).rounded()))%").monospacedDigit()
            if let frame = model.snapshot?.frame {
                Text("\(frame.width) × \(frame.height) px"); Text("\(model.snapshot?.profileName ?? "RGB") · \(model.snapshot?.bitDepth ?? 8) 位")
            }
            Text(readout.coordinates).monospacedDigit()
            Spacer(minLength: 8)
            if model.snapshot?.profileFallback == true { Label("ICC 无效，按 sRGB 处理", systemImage: "exclamationmark.triangle").foregroundStyle(Color(nsColor: WorkspaceColors.warning)) }
            Text(model.busy ? "正在处理…" : model.snapshot?.dirty == true ? "有未保存修改" : "无未保存修改")
            Circle().fill(Color(nsColor: WorkspaceColors.muted)).frame(width: 5, height: 5)
            Text(model.permissions.status)
        }.font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            .padding(.horizontal, 12).frame(height: 24).background(Color(nsColor: WorkspaceColors.panel))
    }
}
