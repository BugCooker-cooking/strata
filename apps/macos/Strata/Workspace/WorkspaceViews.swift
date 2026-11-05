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
                                enabled: !model.busy && tool.available && (![EditorTool.move, .subject].contains(tool) || model.permissions.edits) && (!tool.isBrush || model.permissions.edits) && (!tool.isSelection || model.permissions.selectionEdits), selected: model.tool == tool) { model.selectTool(tool) }
                        }
                    }
                }.padding(.top, 8)
            }
            PaintColorControls(model:model)
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
            case .brush,.eraser:
                BrushToolOptions(model:model)
            case .marquee, .lasso, .selection:
                SelectionToolOptions(model: model)
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
            if model.busy { ProgressView().controlSize(.small); Text("正在处理…"); Button("取消") { model.send(.cancelWork) } }
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
                    case .adjustments: AdjustmentPanel(model: model)
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
                Label(model.selected?.title ?? "文档", systemImage: "square.3.layers.3d").fontWeight(.medium)
                if let layer = model.selected {
                    TextField("图层名称", text: $name).textFieldStyle(.roundedBorder).onSubmit { model.send(.rename(name)) }
                        .disabled(!model.permissions.edits || layer.locks?.all == true).accessibilityLabel("图层名称")
                    caption("位置")
                    HStack {
                        coordinate("X", text: $x); coordinate("Y", text: $y)
                    }
                    caption("外观")
                    BlendOpacityControls(model: model)
                    LayerMetadataControls(model:model)
                    if layer.exposure != nil { ExposureControls(model: model) }
                    if layer.kind?.kind == "adjustment" {
                        if ["brightness_contrast","levels","curves"].contains(layer.kind?.adjustment?["type"].string ?? "") {TonalAdjustmentControls(model:model)}
                        else {PointColorControls(model:model)}
                    }
                    if ["solid","gradient","hue_saturation"].contains(layer.kind?.kind ?? ""){GeneratedLayerControls(model:model)}
                    if !layer.isAdjustment{LayerStyleControls(model:model)}
                    if let mask = layer.mask {
                        Divider(); caption("像素蒙版")
                        Toggle("启用蒙版", isOn: Binding(get: { mask.enabled }, set: { model.send(.command("layer.mask.set_enabled", ["layer": ["id": layer.id], "enabled": $0])) })).disabled(!model.permissions.edits)
                        HStack {
                            Button("反相") { model.send(.command("layer.mask.invert", ["layer": ["id": layer.id]])) }
                            Button(layer.kind?.kind=="pixel" ? "应用" : "应用并栅格化"){model.send(.command("layer.mask.apply",["layer":["id":layer.id],"bake_backdrop":layer.isAdjustment]))}
                            Button("移除") { model.send(.command("layer.mask.remove", ["layer": ["id": layer.id]])) }
                        }.disabled(!model.permissions.edits)
                    }
                    Toggle("显示图层", isOn: Binding(get: { layer.visible }, set: { model.send(.visibility(layer.id, $0)) }))
                        .disabled(!model.permissions.visibility)
                } else { Text("选择一个图层，查看并修改属性。").foregroundStyle(Color(nsColor: WorkspaceColors.secondary)) }
                if let frame = model.snapshot?.frame {
                    Divider(); caption("画板")
                    HStack { Text(frame.name).lineLimit(1); Spacer(); Text("\(frame.width) × \(frame.height) px").monospacedDigit() }
                    Text("\(model.snapshot?.profileName ?? "RGB") · \(model.snapshot?.bitDepth ?? 8) 位 · \(model.snapshot?.resolutionDpi ?? 72) dpi").font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                }
                LuminanceHistogram(values: model.histogram)
                Spacer(minLength: 0)
            }.padding(12)
        }.onChange(of: model.snapshot?.revision) { _, _ in sync() }
            .onChange(of: model.selected?.id) { _, _ in sync() }.onAppear(perform: sync)
    }
    private func caption(_ text: String) -> some View { Text(text).font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
    private func coordinate(_ title: String, text: Binding<String>) -> some View {
        HStack(spacing: 6) { Text(title).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)); TextField(title, text: text).multilineTextAlignment(.trailing).onSubmit(commitPosition); Text("px").foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
            .padding(6).background(RoundedRectangle(cornerRadius: 4).fill(Color(nsColor: WorkspaceColors.input))).disabled(!model.permissions.edits || model.selected?.locks?.position == true || model.selected?.locks?.all == true)
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
                    ForEach(LayerBlendChoice.all,id:\.id){choice in Text(choice.title).tag(choice.id)}
                }.labelsHidden().frame(width: 108)
            }
            HStack(spacing: 8) {
                Text("不透明度").foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                Slider(value: $opacity, in: 0...100, onEditingChanged: { editing in if !editing { model.send(.opacity(opacity / 100)) } }).accessibilityLabel("图层不透明度")
                Text("\(Int(opacity.rounded()))%").monospacedDigit().frame(width: 35, alignment: .trailing)
            }
        }.disabled(!model.permissions.edits || model.selected == nil || model.selected?.locks?.all == true)
            .onAppear { opacity = (model.selected?.opacity ?? 1) * 100 }
            .onChange(of: model.selected?.id) { _, _ in opacity = (model.selected?.opacity ?? 1) * 100 }
            .onChange(of: model.selected?.opacity) { _, value in opacity = (value ?? 1) * 100 }
    }
}

private struct AdjustmentPanel: View {
    @ObservedObject var model: WorkspaceModel
    private let entries = [
        ("exposure","曝光","plusminus"),("brightness_contrast","亮度 / 对比度","sun.max"),("levels","色阶","chart.bar"),
        ("curves","曲线","point.topleft.down.to.point.bottomright.curvepath"),("hue_saturation","色相 / 饱和度","slider.horizontal.3"),
        ("vibrance","自然饱和度","drop"),("white_balance","色温 / 色调","thermometer.medium"),("shadows_highlights","阴影 / 高光","sun.haze"),("color_balance","色彩平衡","circle.lefthalf.filled"),("black_white","黑白","circle.righthalf.filled"),
        ("photo_filter","照片滤镜","camera.filters"),("channel_mixer","通道混合器","slider.horizontal.3"),
        ("invert","反相","circle.dotted"),("posterize","色调分离","square.stack.3d.up"),("threshold","阈值","circle.lefthalf.filled"),
        ("gradient_map","渐变映射","rectangle.lefthalf.filled"),("selective_color","可选颜色","paintpalette"),("desaturate","去色","drop.halffull")]
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("调整图层").fontWeight(.medium)
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())], spacing: 14) {
                    ForEach(entries, id: \.0) { item in
                        VStack(spacing: 7) {
                            WorkspaceIconButton(title: item.1, symbol: item.2, enabled: model.hasDocument && model.permissions.edits) {
                                if item.0=="exposure"{model.send(.command("layer.add_adjustment", ["name": "曝光", "type": "exposure", "stops": 0.5, "mask": model.snapshot?.selection == nil ? "white" : "from_selection"]))}
                                else if item.0=="hue_saturation"{model.send(.command("layer.add_hue_saturation",["name":"色相/饱和度"]))}
                                else {model.send(.command("layer.add_tonal_adjustment",["name":item.1,"adjustment":["type":item.0]]))}
                                model.inspector = .properties
                            }
                            Text(item.1).font(.system(size: 10)).lineLimit(2).multilineTextAlignment(.center)
                        }
                    }
                }
                Text("调整使用当前选区作为蒙版；没有选区时作用于整个合成图。原始图层像素保持可编辑。")
                    .font(.system(size: 11)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).fixedSize(horizontal: false, vertical: true)
                if let subject = model.subjectModel { SubjectModelCard(store: subject, model: model) }
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
                    if let turn = item.turnId, index == 0 || model.snapshot?.history[index - 1].turnId != turn {
                        HStack {
                            Label("AI 回合 · \(String(turn.prefix(8)))", systemImage: "sparkles").font(.system(size: 10, weight: .semibold))
                            Spacer()
                            if let assistant = model.assistant, assistant.canUndoTurn?(turn) == true {
                                Button("整组撤销") { assistant.onUndo?(turn) }.font(.system(size: 10))
                            }
                        }.padding(10).foregroundStyle(Color(nsColor: WorkspaceColors.ai))
                    }
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
                    Text("外部记录 · 不随文档撤销恢复").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).padding(.horizontal, 12)
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
    var layers: [EditorSnapshot.Layer] { model.displayLayers.filter { filter.isEmpty || $0.name.localizedCaseInsensitiveContains(filter) } }
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
                            if layer.isGroup{
                                WorkspaceIconButton(title:"展开或收起 "+layer.name,symbol:model.collapsedGroups.contains(layer.id) ? "chevron.right" : "chevron.down"){
                                    if !model.collapsedGroups.insert(layer.id).inserted{model.collapsedGroups.remove(layer.id)}
                                }
                            }
                            WorkspaceIconButton(title: layer.visible ? "隐藏 \(layer.name)" : "显示 \(layer.name)", symbol: layer.visible ? "eye" : "eye.slash", enabled: model.permissions.visibility) { model.send(.visibility(layer.id, !layer.visible)) }
                            Button { model.send(.selectLayer(layer.id)) } label: {
                                HStack(spacing: 9) {
                                    ZStack {
                                        if layer.isGroup{Image(systemName:"folder").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
                                        else if let image = model.thumbnails[layer.id] { Image(nsImage: image).resizable().scaledToFit() }
                                        else { Image(systemName: layer.isGroup ? "folder" : "photo").foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
                                    }.frame(width: 30, height: 30).background(Color(nsColor: WorkspaceColors.raised)).clipShape(RoundedRectangle(cornerRadius: 4))
                                    VStack(alignment: .leading, spacing: 3) { Text(layer.name).lineLimit(1); Text(layer.title + (layer.mask != nil ? " · 蒙版" : "")).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.muted)) }
                                    if let label=layer.colorLabel{Circle().fill(LayerMetadataControls.labelColor(label)).frame(width:6,height:6)}
                                    Spacer(minLength: 0)
                                    if model.snapshot?.control?.kind == "running", model.snapshot?.activeLayer == layer.id { Image(systemName: "sparkles").foregroundStyle(Color(nsColor: WorkspaceColors.ai)).accessibilityLabel("AI 当前图层") }
                                }.contentShape(Rectangle())
                            }.buttonStyle(.plain).disabled(!model.permissions.selection)
                        }.padding(.leading,CGFloat(model.layerDepth(layer))*12).padding(.horizontal, 4).padding(.vertical, 4)
                            .background(RoundedRectangle(cornerRadius: 5).fill(model.snapshot?.activeLayer == layer.id || model.selectedLayers.contains(layer.id) ? Color(nsColor: WorkspaceColors.accent).opacity(0.20) : .clear))
                            .contextMenu {
                                Button("复制图层") { model.send(.duplicateLayer) }.disabled(model.selected?.id != layer.id || !model.permissions.edits)
                                Button("通过拷贝新建图层"){model.send(.command("layer.via_copy",["layer":["id":layer.id]]))}.disabled(!model.permissions.edits)
                                Button("通过剪切新建图层"){model.send(.command("layer.via_cut",["layer":["id":layer.id]]))}.disabled(!model.permissions.edits || layer.kind?.kind != "pixel")
                                if layer.kind?.kind=="pixel"{Button("背景转普通图层"){model.send(.command("layer.background_to_layer",["layer":["id":layer.id]]))}.disabled(!model.permissions.edits)}
                                Button("将所选图层编组"){model.send(.groupSelected)}.disabled(!model.permissions.edits)
                                Button(layer.clipped == true ? "释放剪贴蒙版" : "创建剪贴蒙版"){model.send(.command("layer.set_clipping",["layer":["id":layer.id],"clipped":layer.clipped != true]))}.disabled(!model.permissions.edits)
                                Button(layer.isAdjustment ? "栅格化并合并下方输入" : "栅格化"){model.send(.command("layer.rasterize",["layer":["id":layer.id],"bake_backdrop":layer.isAdjustment]))}.disabled(!model.permissions.edits || layer.kind?.kind=="pixel")
                                Button("向下合并"){model.send(.command("layer.merge_down",["layer":["id":layer.id]]))}.disabled(!model.permissions.edits)
                                Button("合并可见"){model.send(.command("layer.merge_visible",[:]))}.disabled(!model.permissions.edits)
                                Button("拼合图像"){model.send(.command("layer.flatten",[:]))}.disabled(!model.permissions.edits)
                                Button("盖印可见并保留图层"){model.send(.command("layer.stamp_visible",[:]))}.disabled(!model.permissions.edits)
                                Menu("对齐所选图层"){
                                    ForEach([("左侧","left"),("水平居中","horizontal_center"),("右侧","right"),("顶端","top"),("垂直居中","vertical_center"),("底端","bottom")],id:\.1){title,mode in
                                        Button(title){model.send(.alignSelected(mode))}
                                    }
                                }.disabled(!model.permissions.edits)
                                Menu("分布所选图层"){
                                    ForEach([("水平中心","horizontal_centers"),("垂直中心","vertical_centers"),("水平间距","horizontal_spacing"),("垂直间距","vertical_spacing")],id:\.1){title,mode in
                                        Button(title){model.send(.distributeSelected(mode))}
                                    }
                                }.disabled(!model.permissions.edits||model.selectedLayers.count<3)
                                if layer.isGroup{Button("取消编组"){model.send(.command("layer.ungroup",["layer":["id":layer.id]]))}.disabled(!model.permissions.edits)}
                                Menu("移动到组"){
                                    Button("根层级"){model.send(.command("layer.reparent",["layer":["id":layer.id],"parent":NSNull()]))}
                                    ForEach(model.snapshot?.layers.filter{$0.isGroup&&$0.id != layer.id} ?? [],id:\.id){group in
                                        Button(group.name){model.send(.command("layer.reparent",["layer":["id":layer.id],"parent":["id":group.id]]))}
                                    }
                                }.disabled(!model.permissions.edits)
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
                Menu {
                    ForEach([("白色蒙版", "white"), ("黑色蒙版", "black"), ("来自选区", "from_selection")], id: \.1) { title, source in
                        Button(title) { if let selected = model.selected { model.send(.command("layer.mask.add", ["layer": ["id": selected.id], "source": source])) } }
                            .disabled(source == "from_selection" && model.snapshot?.selection == nil)
                    }
                } label: { Image(systemName: "rectangle.inset.filled") }
                    .menuStyle(.borderlessButton).fixedSize().disabled(!model.permissions.edits || model.selected == nil || model.selected?.mask != nil).help("添加像素蒙版")
                Menu{
                    Button("新建空组"){model.send(.command("layer.create_group",["name":"组"]))}
                    Button("将所选图层编组"){model.send(.groupSelected)}
                }label:{Image(systemName:"folder.badge.plus")}.menuStyle(.borderlessButton).fixedSize().disabled(!model.permissions.edits || !model.hasDocument).help("图层组")
                WorkspaceIconButton(title: "复制图层", symbol: "square.on.square", enabled: model.permissions.edits && model.selected != nil) { model.send(.duplicateLayer) }
                Menu{
                    Button("像素图层…"){model.send(.newLayer)}
                    Button("纯色填充"){let max=model.snapshot?.bitDepth==16 ? 65535 : 255;model.send(.command("layer.fill.solid",["name":"纯色填充","color":[max,max,max,max]]))}
                    Button("渐变填充"){let max=model.snapshot?.bitDepth==16 ? 65535 : 255;model.send(.command("layer.fill.gradient",["name":"渐变填充","start":[0,0,0,max],"end":[max,max,max,max]]))}
                }label:{Image(systemName:"plus.square")}.menuStyle(.borderlessButton).fixedSize().disabled(!model.permissions.edits || !model.hasDocument).help("新建图层")
                WorkspaceIconButton(title: "置入图片 (⇧⌘P)", symbol: "photo.badge.plus", enabled: model.permissions.edits && model.hasDocument) { model.send(.place) }
                WorkspaceIconButton(title: "删除图层", symbol: "trash", enabled: model.permissions.edits && model.selected != nil && model.selected?.locks?.all != true) { model.send(.deleteLayer) }
            }.padding(5)
        }
    }
}

struct AssistantPanel: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        if let assistant = model.assistant { ConnectedAssistantPanel(model: assistant, workspace: model) }
        else { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
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
            if !model.recoveryStatus.isEmpty { Text(model.recoveryStatus).help(model.recoveryError).lineLimit(1) }
            if model.snapshot?.profileFallback == true { Label("ICC 无效，按 sRGB 处理", systemImage: "exclamationmark.triangle").foregroundStyle(Color(nsColor: WorkspaceColors.warning)) }
            Text(model.busy ? "正在处理…" : model.snapshot?.dirty == true ? "有未保存修改" : "无未保存修改")
            Circle().fill(Color(nsColor: model.snapshot?.control?.kind == "paused" ? WorkspaceColors.warning : model.snapshot?.control?.kind == "running" ? WorkspaceColors.ai : model.assistant?.configured == true ? .systemGreen : WorkspaceColors.muted)).frame(width: 5, height: 5)
            Text(model.snapshot?.control?.kind == "user" ? (model.assistant?.status ?? model.permissions.status) : model.permissions.status)
        }.font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            .padding(.horizontal, 12).frame(height: 24).background(Color(nsColor: WorkspaceColors.panel))
    }
}
