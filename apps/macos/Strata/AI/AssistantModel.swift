import AppKit
import Combine
import Foundation
import UniformTypeIdentifiers

struct AIExecution: Identifiable {
    let id: String
    var status = "执行中"
    var steps: [AIJSON] = []
    var inputTokens: Double = 0
    var outputTokens: Double = 0
    var elapsed: Double = 0
    var externalWrites = 0
}

@MainActor
final class AssistantModel: ObservableObject {
    @Published var messages: [AIChatMessage] = []
    @Published var conversations: [AIConversationSummary] = []
    @Published var executions: [AIExecution] = []
    @Published var draft = ""
    @Published var answer = ""
    @Published var askFirst = true
    @Published var running = false
    @Published var paused: AIJSON?
    @Published var error = ""
    @Published var settingsVisible = false
    @Published var testing = false
    @Published var connectionStatus = ""
    let settings: AISettingsStore
    private let engine: StrataEngine
    private var task: Task<Void, Never>?
    private(set) var turn: String?
    private var document: String?
    private var contextApprovals: [String: Bool] = [:]
    private var streamMessage: UUID?
    private var panel: NSSavePanel?
    private var subscriptions: Set<AnyCancellable> = []
    private var activeConfiguration: AIConfiguration?
    private var generation = UUID()
    private var alertWindow: NSWindow?
    private var selectedResourceURL: URL?
    var window: (() -> NSWindow?)?
    var onStateChanged: (() -> Void)?
    var onUndo: ((String) -> Void)?
    var canUndoTurn: ((String) -> Bool)?
    var onDocumentFile: ((URL?, String) -> Void)?

    init(engine: StrataEngine, settings: AISettingsStore = .shared) {
        self.engine = engine; self.settings = settings
        settings.objectWillChange.sink { [weak self] in self?.objectWillChange.send() }.store(in: &subscriptions)
    }
    var configured: Bool { settings.current?.toolsVerified == true }
    var status: String { running ? (paused == nil ? "执行中" : "等待你的决定") : configured ? "已连接 · 空闲" : "未接入模型" }
    func bind(document next: String?, inheritedTurn: String? = nil) {
        guard document != next else { return }
        if running, let turn, inheritedTurn == turn { document = next; contextApprovals = [:]; persist(); return }
        if running { stop() }
        document = next; contextApprovals = [:]; executions = []; messages = []; conversations=[]
        if let next {
            do { let store=try AIConversationStore.applicationStore();messages=try store.load(document:next);conversations=try store.list(document:next) }
            catch { self.error = "对话记录读取失败：\(error.localizedDescription)" }
        }
    }
    private func persist() {
        guard let document else { return }
        do { let store=try AIConversationStore.applicationStore();try store.save(messages,document:document);conversations=try store.list(document:document) }
        catch { self.error = "对话记录未保存：\(error.localizedDescription)" }
    }
    func newConversation() {
        guard !running else { return }
        do {
            if let document {try AIConversationStore.applicationStore().newConversation(document:document)}
            messages=[];executions=[];error="";persist()
        }catch{self.error=error.localizedDescription}
    }
    func activateConversation(_ id:UUID) {
        guard !running,let document else{return}
        do {
            let store=try AIConversationStore.applicationStore();try store.activate(id,document:document)
            messages=try store.load(document:document);conversations=try store.list(document:document);executions=[]
        }catch{self.error=error.localizedDescription}
    }
    func testAndSave(_ draft: AIConfiguration, key: String) async {
        guard !testing else { return }; testing = true; connectionStatus = "正在验证工具调用…"; error = ""
        defer { testing = false }
        do {
            var config = draft; config.vision = false; config.toolsVerified = false
            let configJSON = try config.json(); _ = try config.validated()
            let credential = try await settings.credentialAsync(for: config, replacement: key)
            let transport = AITransport()
            let toolWire = try AIWireRequest.decode(aiProbeRequest(configJson: configJSON, vision: false))
            let toolData = try await transport.collect(toolWire, key: credential)
            guard try aiProbeResult(configJson: configJSON, response: toolData, vision: false) else { throw AIClientError.message("连接成功，但模型未完成工具调用探测。请选择支持工具的模型。") }
            config.toolsVerified = true; connectionStatus = "工具调用通过；正在检测看图能力…"
            do {
                let visionWire = try AIWireRequest.decode(aiProbeRequest(configJson: configJSON, vision: true))
                let visionData = try await transport.collect(visionWire, key: credential)
                config.vision = try aiProbeResult(configJson: configJSON, response: visionData, vision: true)
            } catch { config.vision = false }
            try settings.save(config, key: key)
            connectionStatus = config.vision ? "工具调用与看图探测通过" : "工具调用通过 · 画布仅提供文字统计"
            settingsVisible = false; onStateChanged?()
        } catch { self.error = error.localizedDescription; connectionStatus = "连接未通过" }
    }
    func listModels(_ draft: AIConfiguration, key: String) async -> [String] {
        guard !testing else { return [] }; testing = true; error = ""; connectionStatus = "正在读取模型列表…"
        defer { testing = false }
        do {
            var config = draft
            if config.model.isEmpty { config.model = "model-list-probe" }
            let request = try AIWireRequest.decode(aiModelsRequest(configJson: config.json()))
            let credential = try await settings.credentialAsync(for: config, replacement: key)
            let data = try await AITransport().collect(request, key: credential)
            let response = try AIJSON.decode(String(decoding: data, as: UTF8.self))
            let models = Array(Set(response["data"].array.compactMap { $0["id"].string }.filter { !$0.isEmpty && $0.utf8.count <= 256 })).sorted()
            guard !models.isEmpty else { throw AIClientError.message("接口没有返回可用模型列表；可以手工填写模型名称。") }
            connectionStatus = "已读取 \(models.count) 个模型"; return Array(models.prefix(500))
        } catch { self.error = error.localizedDescription; connectionStatus = "模型列表读取失败，可手工填写"; return [] }
    }
    func send() {
        guard !running, configured, let config = settings.current else { return }
        let boundDocument = document
        let documentScope = document ?? "empty:\((try? EditorSnapshot.decode(engine.snapshotJson()).sessionId) ?? "session")"
        let prompt = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else { return }
        running = true; error = ""; paused = nil; draft = ""
        generation = UUID(); let generation = generation
        task = Task { [weak self] in
            guard let self else { return }
            do {
                let configJSON = try config.json()
                let identity = try config.validated()["identity"].string ?? ""
                let approvalKey = documentScope + "|" + identity
                var shareCanvas = contextApprovals[approvalKey]
                if shareCanvas == nil {
                    let destination = try config.validated()["url"].string ?? config.endpoint
                    let connection = destination.hasPrefix("http://") && config.kind != .local ? "\n此连接使用明文 HTTP，密钥与上下文不会加密。" : ""
                    let choice = await alert(title: "授权本轮文档上下文", message: "目的地：\(destination)\n模型：\(config.model)\(connection)\n\n将发送你的消息和当前文档的尺寸、色彩、可见图层名称及历史摘要。你可以同时允许模型按需读取当前画板的可见合成图；隐藏图层内容和其他文档不会自动发送。", buttons: config.vision ? ["共享可见画布与元数据", "仅共享元数据", "取消"] : ["共享元数据与画布统计", "取消"])
                    try Task.checkCancellation()
                    if (config.vision && ![NSApplication.ModalResponse.alertFirstButtonReturn, .alertSecondButtonReturn].contains(choice)) || (!config.vision && choice != .alertFirstButtonReturn) { finishLocal(status: "未授权上下文"); draft = prompt; return }
                    shareCanvas = config.vision && choice == .alertFirstButtonReturn
                    contextApprovals[approvalKey] = shareCanvas
                }
                guard self.document == boundDocument else { throw AIClientError.message("活动文档已改变，请重新发送。") }
                let credential = try await settings.credentialAsync(for: config)
                try Task.checkCancellation()
                guard self.document==boundDocument,self.generation==generation else{throw CancellationError()}
                let prior = messages.filter { ["user", "assistant"].contains($0.role) && !$0.text.isEmpty }.suffix(40).map {
                    AIJSON.object(["role": .string($0.role), "text": .string(String($0.text.prefix(8000)))])
                }
                let priorJSON = try AIJSON.array(prior).encoded()
                let options = try commandJSON(["mode": askFirst ? "ask_first" : "direct", "max_steps": 50, "request_timeout_seconds": 60, "turn_timeout_seconds": 300])
                // No suspension between the final document check and grant creation.
                let turn = try engine.aiBegin(configJson: configJSON, optionsJson: options, prompt: prompt, shareCanvas: shareCanvas ?? false, priorJson: priorJSON)
                self.turn = turn; activeConfiguration = config; executions.append(AIExecution(id: turn))
                messages.append(AIChatMessage(role: "user", text: prompt, turn: turn)); persist(); onStateChanged?()
                try await drive(config: config, credential: credential, turn: turn)
            } catch is CancellationError { }
            catch { guard self.generation == generation, !Task.isCancelled else { return }; if let turn { try? engine.aiStop(turnId: turn) }; self.error = error.localizedDescription; finishLocal(status: "失败，已释放操作权") }
        }
    }
    private func drive(config: AIConfiguration, credential: String, turn: String) async throws {
        let engine = engine
        while !Task.isCancelled, self.turn == turn {
            let raw = try await aiEngineWork { try engine.aiPoll(turnId: turn) }
            let step = try AIJSON.decode(raw)
            switch step["kind"].string {
            case "request":
                guard let requestID = step["request_id"].string else { throw AIClientError.message("缺少模型请求标识。") }
                let wire = try AIWireRequest.decode(step["request"].encoded())
                let message = AIChatMessage(role: "assistant", text: "", turn: turn)
                streamMessage = message.id; messages.append(message)
                do {
                    try await AITransport().send(wire, key: credential, timeout: step["timeout_seconds"].number ?? 60) { [weak self] chunk in
                        let delta = try await aiEngineWork { try engine.aiFeed(requestId: requestID, bytes: chunk) }
                        await self?.appendDelta(delta, turn: turn)
                    }
                    try Task.checkCancellation()
                    let finalText = try await aiEngineWork { try engine.aiFinishResponse(requestId: requestID) }
                    if let id = streamMessage, let index = messages.firstIndex(where: { $0.id == id }) { messages[index].text = finalText }
                } catch {
                    try Task.checkCancellation()
                    let retryable = (error as? AIClientError)?.retryable == true || ((error as? URLError).map { [.networkConnectionLost, .cannotConnectToHost, .notConnectedToInternet, .timedOut].contains($0.code) } ?? false)
                    try await aiEngineWork { try engine.aiRequestFailed(requestId: requestID, retryable: retryable) }
                    if retryable { messages.append(AIChatMessage(role: "system", text: "网络请求失败，正在有限重试；已提交的步骤不会重复执行。", turn: turn)) }
                }
                messages.removeAll { $0.role == "assistant" && $0.text.isEmpty }; streamMessage = nil; persist()
            case "tool": record(step, turn: turn)
            case "paused":
                if step["reason"].string=="clipboard_write"{try await completeClipboardWrite(step,turn:turn);continue}
                paused = step; answer = ""; onStateChanged?(); persist(); return
            case "finished":
                if let index = executions.firstIndex(where: { $0.id == turn }) {
                    executions[index].inputTokens = step["usage"]["input_tokens"].number ?? 0
                    executions[index].outputTokens = step["usage"]["output_tokens"].number ?? 0
                    executions[index].elapsed = (step["elapsed_ms"].number ?? 0) / 1000
                    executions[index].externalWrites = Int(step["external_writes"].number ?? 0)
                }
                finishLocal(status: step["status"].string == "completed" ? "已完成" : "已停止（\(step["status"].string ?? "unknown")）"); return
            default: throw AIClientError.message("无法识别 AI 回合状态。")
            }
        }
    }
    private func appendDelta(_ delta: String, turn: String) {
        guard self.turn == turn, let id = streamMessage, let index = messages.firstIndex(where: { $0.id == id }) else { return }
        messages[index].text += delta
    }
    private func completeClipboardWrite(_ pending:AIJSON,turn:String)async throws{
        guard let resource=pending["operation"]["resource"].string else{throw AIClientError.message("剪贴板写入资源缺失。")}
        let engine=engine;var success=false
        do{
            let transfer=try await aiEngineWork{try engine.clipboardExport(resource:resource,turnId:turn)}
            try Task.checkCancellation();guard self.turn==turn else{throw CancellationError()}
            try NativeClipboard.write(transfer)
            try engine.clipboardConfirmWrite(resource:resource,turnId:turn);success=true
        }catch is CancellationError{throw CancellationError()}
        catch{self.error=error.localizedDescription}
        let response=try commandJSON(["question_id":pending["question_id"].string ?? "","approved":success,"resource":resource])
        let raw=try await aiEngineWork{try engine.aiRespond(answerJson:response)}
        record(try AIJSON.decode(raw),turn:turn);persist()
    }
    private func record(_ step: AIJSON, turn: String) {
        if let index = executions.firstIndex(where: { $0.id == turn }) { executions[index].steps.append(step) }
        onStateChanged?()
    }
    func respond(approved: Bool, text: String? = nil) {
        guard running, let pending = paused, let turn, let config = activeConfiguration else { return }
        let generation = generation
        // A paused turn keeps its original destination and model. Switching
        // models is disabled in the panel until this turn finishes.
        paused = nil
        task = Task { [weak self] in
            guard let self else { return }
            do {
                let credential = try await settings.credentialAsync(for: config)
                try Task.checkCancellation()
                guard self.turn==turn,self.generation==generation else{throw CancellationError()}
                var resource: String?
                if pending["reason"].string == "file_resource", approved { resource = try await pickResource(pending, turn: turn) }
                if pending["reason"].string=="clipboard_read",approved{
                    do{resource=try await NativeClipboard.read(engine:engine,turn:turn)}catch{self.error=error.localizedDescription}
                }
                try Task.checkCancellation()
                let response = try commandJSON(["question_id": pending["question_id"].string ?? "", "answer": text ?? answer,
                                               "approved": approved && (!["file_resource","clipboard_read"].contains(pending["reason"].string ?? "") || resource != nil), "resource": resource as Any? ?? NSNull()])
                let engine = engine
                let raw = try await aiEngineWork { try engine.aiRespond(answerJson: response) }
                let step = try AIJSON.decode(raw)
                if step["kind"].string=="tool"{record(step, turn: turn)}
                if step["result"]["result"]["document_id"] != .null {
                    onDocumentFile?(selectedResourceURL, pending["operation"]["command"].string ?? "file.new")
                }
                if let command = pending["operation"]["command"].string, ["file.save", "file.save_as"].contains(command), step["result"]["result"]["status"].string == "completed" { onDocumentFile?(selectedResourceURL, command) }
                selectedResourceURL = nil
                if pending["reason"].string == "question" { messages.append(AIChatMessage(role: "user", text: text ?? answer, turn: turn)) }
                answer = ""; persist(); try await drive(config: config, credential: credential, turn: turn)
            } catch is CancellationError { }
            catch { guard self.generation == generation, !Task.isCancelled else { return }; try? engine.aiStop(turnId: turn); self.error = error.localizedDescription; finishLocal(status: "失败，已释放操作权") }
        }
    }
    func stop() {
        generation = UUID()
        task?.cancel(); task = nil; panel?.cancel(nil); panel = nil
        if let alertWindow, let window = window?(), window.attachedSheet === alertWindow { window.endSheet(alertWindow, returnCode: .cancel) }
        if let turn { try? engine.aiStop(turnId: turn) }
        messages.append(AIChatMessage(role: "system", text: "本轮已停止。已提交的文档修改保留，可从历史撤销；已完成的保存/导出和系统剪贴板写入不会随文档撤销恢复。", turn: turn))
        finishLocal(status: "已停止")
    }
    private func finishLocal(status: String) {
        if let turn, let index = executions.firstIndex(where: { $0.id == turn }) { executions[index].status = status }
        running = false; paused = nil; streamMessage = nil; turn = nil; activeConfiguration = nil; persist(); onStateChanged?()
    }
    private func alert(title: String, message: String, buttons: [String]) async -> NSApplication.ModalResponse {
        guard let window = window?() else { return .alertThirdButtonReturn }
        let alert = NSAlert(); alert.messageText = title; alert.informativeText = message
        buttons.forEach { alert.addButton(withTitle: $0) }
        alertWindow = alert.window
        let response = await withCheckedContinuation { continuation in alert.beginSheetModal(for: window) { continuation.resume(returning: $0) } }
        alertWindow = nil; return response
    }
    private func pickResource(_ pending: AIJSON, turn: String) async throws -> String? {
        guard let window = window?() else { return nil }
        let operation = pending["operation"]; let command = operation["command"].string ?? ""
        if ["file.place", "file.open", "file.recover"].contains(command) {
            let picker = NSOpenPanel(); picker.allowedContentTypes = [.png, .jpeg, .tiff, .webP, .heic, .gif, .bmp, UTType(filenameExtension: "strata") ?? .data]; picker.allowsMultipleSelection = false
            picker.message = "授权本轮 AI 读取并置入所选图片"; panel = picker
            let choice = await withCheckedContinuation { continuation in picker.beginSheetModal(for: window) { continuation.resume(returning: $0) } }
            panel = nil; guard choice == .OK, let url = picker.url, self.turn == turn else { return nil }
            selectedResourceURL = url
            return try engine.aiGrantRead(turnId: turn, path: url.path)
        }
        let format:String
        if command=="file.quick_export"{
            let preset=try QuickExportPreferences().preset(engine)
            guard let value=operation["params"]["preset_version"].number,UInt64(exactly:value)==preset.version else{throw AIClientError.message("快速导出预设已改变，请重新读取预设并发起导出。")}
            format=try QuickExportPreferences().format(preset)
        }else{format = command == "file.export" ? operation["params"]["format"].string ?? "png" : "strata"}
        guard ["png", "jpeg", "jpg", "webp", "tiff", "strata"].contains(format) else { throw AIClientError.message("请先选择支持的导出格式。") }
        let picker = NSSavePanel(); picker.allowedContentTypes = [UTType(filenameExtension: format) ?? .data]
        picker.nameFieldStringValue = "Strata.\(format)"; picker.message = "授权 AI 的这一次文件写入；目标文件不受文档撤销保护。"; panel = picker
        let choice = await withCheckedContinuation { continuation in picker.beginSheetModal(for: window) { continuation.resume(returning: $0) } }
        panel = nil; guard choice == .OK, let url = picker.url, self.turn == turn else { return nil }
        let exists = FileManager.default.fileExists(atPath: url.path)
        if exists {
            let choice = await alert(title: "确认覆盖文件", message: "本次 AI 操作将替换：\n\(url.path)\n\n撤销图层编辑不会还原这个磁盘文件。", buttons: ["覆盖", "取消"])
            guard choice == .alertFirstButtonReturn, self.turn == turn else { return nil }
        }
        selectedResourceURL = url
        return try engine.aiGrantWrite(turnId: turn, path: url.path, overwrite: exists)
    }
}
