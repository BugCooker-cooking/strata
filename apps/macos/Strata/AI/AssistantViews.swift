import SwiftUI

struct ConnectedAssistantPanel: View {
    @ObservedObject var model: AssistantModel
    @ObservedObject var workspace: WorkspaceModel
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 7) {
                Image(systemName: "sparkles").foregroundStyle(Color(nsColor: WorkspaceColors.ai)); Text(L10n.text("AI 助手")).fontWeight(.semibold)
                Spacer()
                WorkspaceIconButton(title: L10n.text("新对话"), symbol: "square.and.pencil", enabled: !model.running) { model.newConversation() }
                Menu {
                    ForEach(model.conversations){conversation in
                        Button{model.activateConversation(conversation.id)}label:{
                            if conversation.current {Label(conversation.title,systemImage:"checkmark")}else{Text(conversation.title)}
                        }
                    }
                }label:{Image(systemName:"clock.arrow.circlepath")}
                    .menuStyle(.borderlessButton).fixedSize().disabled(model.running||model.conversations.isEmpty).help(L10n.text("文档对话记录"))
                WorkspaceIconButton(title: L10n.text("模型设置"), symbol: "slider.horizontal.3", enabled: !model.running) { model.settingsVisible = true }
                WorkspaceIconButton(title: L10n.text("收起 AI 助手 (⌘J)"), symbol: "sidebar.right") { workspace.send(.assistant) }
            }.padding(.horizontal, 12).frame(height: 44)
            Divider()
            if !model.settings.configurations.isEmpty {
                HStack {
                    Picker(L10n.text("模型"), selection: Binding(get: { model.settings.selected }, set: { model.settings.selected = $0 })) {
                        ForEach(model.settings.configurations) { config in Text(config.title).tag(Optional(config.id)) }
                    }.labelsHidden().disabled(model.running)
                    Text(model.status).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                }.padding(10)
                Divider()
            }
            ScrollViewReader { scroll in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 14) {
                        if !model.configured { onboarding }
                        if let subject = workspace.subjectModel { SubjectModelCard(store: subject, model: workspace) }
                        ForEach(model.messages) { message in
                            VStack(alignment: message.role == "user" ? .trailing : .leading, spacing: 5) {
                                Text(message.role == "user" ? L10n.text("你") : message.role == "assistant" ? "AI" : L10n.text("系统"))
                                    .font(.system(size: 10, weight: .semibold)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                                Text(message.text.isEmpty ? L10n.text("正在等待模型…") : message.text)
                                    .textSelection(.enabled).lineSpacing(3).padding(10)
                                    .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: message.role == "user" ? WorkspaceColors.accent : WorkspaceColors.raised).opacity(message.role == "user" ? 0.18 : 1)))
                            }.frame(maxWidth: .infinity, alignment: message.role == "user" ? .trailing : .leading).id(message.id)
                        }
                        ForEach(model.executions) { execution in AIExecutionCard(execution: execution, canUndo: !model.running && model.canUndoTurn?(execution.id) == true) { model.onUndo?(execution.id) } }
                        if let pending = model.paused { question(pending) }
                        if !model.error.isEmpty {
                            Label(model.error, systemImage: "exclamationmark.triangle").foregroundStyle(Color(nsColor: WorkspaceColors.danger)).textSelection(.enabled)
                        }
                        Color.clear.frame(height: 1).id("end")
                    }.padding(12)
                }.onChange(of: model.messages.count) { _, _ in scroll.scrollTo("end", anchor: .bottom) }
                    .onChange(of: model.paused != nil) { _, _ in scroll.scrollTo("end", anchor: .bottom) }
            }
            Divider()
            VStack(alignment: .leading, spacing: 8) {
                Label(workspace.selected?.name ?? L10n.text("当前文档"), systemImage: "square.3.layers.3d").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).lineLimit(1)
                if let selection=workspace.selectionSummary {Label(selection,systemImage:"rectangle.dashed").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary)).lineLimit(1)}
                TextField(L10n.text("描述你想要的效果…"), text: $model.draft, axis: .vertical).lineLimit(2...5).textFieldStyle(.plain)
                    .disabled(model.running || !model.configured).accessibilityLabel(L10n.text("AI 消息"))
                    .onSubmit { model.send() }
                HStack {
                    Toggle(L10n.text("先问我"), isOn: $model.askFirst).toggleStyle(.checkbox).font(.system(size: 10)).disabled(model.running)
                    Spacer()
                    if model.running {
                        Button(L10n.text("停止"), systemImage: "stop.circle.fill") { model.stop() }.tint(Color(nsColor: WorkspaceColors.danger)).keyboardShortcut(".", modifiers: .command)
                    } else {
                        Button(L10n.text("发送"), systemImage: "arrow.up.circle.fill") { model.send() }
                            .disabled(workspace.busy || !model.configured || model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }
                Text(L10n.text("文档编辑可撤销；已写入的文件不随撤销恢复。")).font(.system(size: 9)).foregroundStyle(Color(nsColor: WorkspaceColors.muted))
            }.padding(12).background(Color(nsColor: WorkspaceColors.input).opacity(0.5))
        }.font(.system(size: 12)).foregroundStyle(Color(nsColor: WorkspaceColors.text)).background(Color(nsColor: WorkspaceColors.panel))
            .sheet(isPresented: $model.settingsVisible) { AISettingsView(model: model, seed: model.settings.current ?? AIConfiguration()) }
    }
    private var onboarding: some View {
        VStack(alignment: .leading, spacing: 16) {
            Image(systemName: "sparkles").font(.system(size: 26)).foregroundStyle(Color(nsColor: WorkspaceColors.ai))
            Text(L10n.text("接入你的模型")).font(.system(size: 17, weight: .semibold))
            Text(L10n.text("连接 Anthropic、OpenAI 兼容服务或本机 Ollama / LM Studio，让 AI 使用同一套图层、历史与文件命令。"))
                .foregroundStyle(Color(nsColor: WorkspaceColors.secondary)).lineSpacing(4)
            Button(L10n.text("设置模型并测试连接")) { model.settingsVisible = true }.buttonStyle(.borderedProminent)
            Label(L10n.text("密钥仅存本机钥匙串"), systemImage: "lock.shield")
            Label(L10n.text("每个文档和提供方单独授权上下文"), systemImage: "photo.badge.checkmark")
            Label(L10n.text("不接模型也能使用手工编辑"), systemImage: "cursorarrow")
        }.padding(10).frame(maxWidth: .infinity, alignment: .leading)
    }
    private func question(_ pending: AIJSON) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(L10n.text("AI 等你决定"), systemImage: "pause.circle").fontWeight(.semibold)
            Text(pending["question"].string ?? "")
            if pending["operation"] != .null {
                DisclosureGroup(L10n.text("查看操作与参数")) { Text((try? pending["operation"].encoded()) ?? "").font(.system(size: 10, design: .monospaced)).textSelection(.enabled) }
            }
            if pending["reason"].string == "question" {
                ForEach(pending["options"].array.indices, id: \.self) { index in
                    let option = pending["options"].array[index].string ?? ""
                    Button(option) { model.respond(approved: false, text: option) }.buttonStyle(.bordered)
                }
                TextField(L10n.text("输入回答…"), text: $model.answer, axis: .vertical).textFieldStyle(.roundedBorder)
                Button(L10n.text("继续")) { model.respond(approved: false) }.disabled(model.answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            } else {
                HStack {
                    Button(pending["reason"].string == "file_resource" ? L10n.text("选择文件并授权…") : L10n.text("允许")) { model.respond(approved: true) }.buttonStyle(.borderedProminent)
                    Button(L10n.text("拒绝此操作")) { model.respond(approved: false) }
                }
            }
            Text(L10n.text("暂停期间可以缩放查看、撤销与切换图层显隐；恢复时 AI 会收到最新状态。"))
                .font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
        }.padding(12).background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: WorkspaceColors.warning).opacity(0.12)))
    }
}

private struct AIExecutionCard: View {
    let execution: AIExecution
    let canUndo: Bool
    let undo: () -> Void
    var body: some View {
        DisclosureGroup {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(execution.steps.enumerated()), id: \.offset) { index, step in
                    VStack(alignment: .leading, spacing: 3) {
                        Label("\(index + 1). \(stepTitle(step))", systemImage: step["result"]["error"] == .null ? "checkmark.circle" : "exclamationmark.circle")
                        Text(step["arguments"]["id"].string ?? step["name"].string ?? "").font(.system(size: 10, design: .monospaced)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                        DisclosureGroup(L10n.format("参数与结果 · {0} ms", String(describing: Int(step["elapsed_ms"].number ?? 0)))) {
                            Text(String(((try? step["arguments"].encoded()) ?? "").prefix(8000))).textSelection(.enabled)
                            Text(String(((try? step["result"].encoded()) ?? "").prefix(8000))).textSelection(.enabled)
                        }.font(.system(size: 10))
                    }
                }
                Text(L10n.format("{0} 输入 / {1} 输出 token · {2} 秒", String(describing: execution.inputTokens.formatted()), String(describing: execution.outputTokens.formatted()), String(describing: execution.elapsed.formatted(.number.precision(.fractionLength(1))))))
                    .font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                if execution.externalWrites > 0 { Text(L10n.format("已完成 {0} 次文件写入，不随文档撤销恢复。", String(describing: execution.externalWrites))).font(.system(size: 10)) }
                Button(L10n.text("整组撤销文档编辑"), action: undo).disabled(!canUndo)
            }.padding(.top, 6)
        } label: { Label(L10n.format("{0} · {1} 步", String(describing: execution.status), String(describing: execution.steps.count)), systemImage: "square.stack.3d.up") }
            .padding(10).background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: WorkspaceColors.ai).opacity(0.08)))
    }
    private func stepTitle(_ step: AIJSON) -> String {
        switch step["name"].string {
        case "browse_catalog": L10n.format("目录 › {0}", String(describing: step["arguments"]["path"].string ?? L10n.text("根目录")))
        case "search_catalog": L10n.format("搜索 › {0}", String(describing: step["arguments"]["query"].string ?? ""))
        case "describe_commands": L10n.text("读取命令说明")
        case "invoke_command": commandTitle(step["arguments"]["id"].string ?? "")
        case "invoke_batch": L10n.text("原子批量编辑")
        case "inspect_document": L10n.text("读取文档结构")
        case "view_canvas": L10n.text("读取授权画布")
        case "ask_user": L10n.text("询问用户")
        default: step["name"].string ?? ""
        }
    }
}

private struct AISettingsView: View {
    @ObservedObject var model: AssistantModel
    @State var seed: AIConfiguration
    @State private var key = ""
    @State private var availableModels: [String] = []
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(L10n.text("模型设置")).font(.title2).fontWeight(.semibold)
            Form {
                Picker(L10n.text("提供方"), selection: $seed.kind) { ForEach(AIProvider.allCases, id: \.self) { Text($0.title).tag($0) } }
                    .onChange(of: seed.kind) { _, value in seed.endpoint = value.defaultEndpoint; seed.toolsVerified = false; key = "" }
                TextField(L10n.text("服务地址"), text: $seed.endpoint).textFieldStyle(.roundedBorder)
                TextField(L10n.text("模型名称"), text: $seed.model).textFieldStyle(.roundedBorder)
                SecureField(seed.kind == .local ? L10n.text("API Key（本地可留空）") : L10n.text("API Key（留空使用此地址和模型已存密钥）"), text: $key).textFieldStyle(.roundedBorder)
                if seed.endpoint.lowercased().hasPrefix("http://"), seed.kind != .local {
                    Toggle(L10n.text("允许明文 HTTP（密钥与上下文不会加密）"), isOn: $seed.allowInsecureHTTP).foregroundStyle(.orange)
                }
                HStack {
                    Button(L10n.text("读取可用模型")) { Task { availableModels = await model.listModels(seed, key: key) } }.disabled(model.testing)
                    if !availableModels.isEmpty {
                        Picker(L10n.text("选择模型"), selection: $seed.model) { Text(L10n.text("请选择")).tag(""); ForEach(availableModels, id: \.self) { Text($0).tag($0) } }
                    }
                }
            }
            Text(L10n.text("密钥存入本机钥匙串，按提供方、地址与模型隔离。HTTPS 默认开启；远程明文 HTTP 需明确允许。本地服务仅限回环地址。连接测试只发送固定的文字、工具探测和测试色块。"))
                .font(.caption).foregroundStyle(.secondary)
            if !model.connectionStatus.isEmpty { Text(model.connectionStatus).font(.caption) }
            if !model.error.isEmpty { Text(model.error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                if model.settings.configurations.contains(where: { $0.id == seed.id }) {
                    Button(L10n.text("添加另一个模型")) { seed = AIConfiguration(); key = "" }.disabled(model.testing)
                    Button(L10n.text("删除连接")) { do { try model.settings.remove(seed); seed = AIConfiguration(); key = "" } catch { model.error = error.localizedDescription } }.disabled(model.testing)
                }
                Spacer()
                Button(L10n.text("取消")) { model.settingsVisible = false }.disabled(model.testing)
                Button(model.testing ? L10n.text("测试中…") : L10n.text("连接、测试并保存")) { Task { await model.testAndSave(seed, key: key); key = "" } }
                    .buttonStyle(.borderedProminent).disabled(model.testing || seed.model.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }.padding(24).frame(width: 560).interactiveDismissDisabled(model.testing)
    }
}
