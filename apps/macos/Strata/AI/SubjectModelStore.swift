import AppKit
import Foundation

struct SubjectModelAsset: Decodable, Sendable {
    let id: String; let version: String; let name: String; let license: String
    let source: String; let url: String; let sha256: String; let maxBytes: UInt64
    static func current() throws -> Self {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(Self.self, from: Data(subjectModelAssetJson().utf8))
    }
}

/// Stage beside the destination so publishing uses one atomic rename. Bound
/// the copy as well as the final digest; a source may change while importing.
enum SubjectModelInstallation {
    static func stage(source: URL, destination: URL, maximumBytes: UInt64) throws -> URL {
        let attributes = try source.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard attributes.isRegularFile == true, attributes.isSymbolicLink != true else { throw AIClientError.message(L10n.text("请选择普通 ONNX 模型文件。")) }
        let directory = destination.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temporary = directory.appendingPathComponent("import-\(UUID().uuidString).download")
        var complete = false
        defer { if !complete { try? FileManager.default.removeItem(at: temporary) } }
        guard FileManager.default.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw AIClientError.message(L10n.text("无法创建模型导入文件。")) }
        let input = try FileHandle(forReadingFrom: source), output = try FileHandle(forWritingTo: temporary)
        defer { try? input.close(); try? output.close() }
        var total: UInt64 = 0
        while let data = try input.read(upToCount: 65536), !data.isEmpty {
            total += UInt64(data.count)
            guard total <= maximumBytes else { throw AIClientError.message(L10n.text("模型文件超过支持的大小限制。")) }
            try output.write(contentsOf: data)
        }
        try output.synchronize()
        try verifySubjectModel(path: temporary.path)
        complete = true
        return temporary
    }
    static func publish(_ temporary: URL, to destination: URL) throws {
        if FileManager.default.fileExists(atPath: destination.path) { _ = try FileManager.default.replaceItemAt(destination, withItemAt: temporary) }
        else { try FileManager.default.moveItem(at: temporary, to: destination) }
    }
}

private final class SubjectEngineReference {
    weak var engine: StrataEngine?
    init(_ engine: StrataEngine) { self.engine = engine }
}

@MainActor
final class SubjectModelStore: ObservableObject {
    static let shared = SubjectModelStore()
    @Published private(set) var installed = false
    @Published private(set) var downloading = false
    @Published private(set) var progress = 0.0
    @Published private(set) var status = L10n.text("主体模型尚未下载")
    @Published private(set) var backend = "cpu"
    @Published var error = ""
    private var download: Task<Void, Never>?
    private var waiters: [@MainActor () -> Void] = []
    private var engines: [SubjectEngineReference] = []
    private(set) var modelURL: URL?
    init() {
        backend = UserDefaults.standard.string(forKey: "subject.backend") == "core_ml" ? "core_ml" : "cpu"
        do {
            let root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            modelURL = root.appendingPathComponent("Strata/Models/u2netp-rembg-v0.0.0.onnx")
            if let modelURL, (try? verifySubjectModel(path: modelURL.path)) != nil { installed = true; installedStatus() }
        } catch { self.error = error.localizedDescription }
    }
    func configure(_ engine: StrataEngine) throws {
        guard let library = Bundle.main.privateFrameworksURL?.appendingPathComponent("libstrata_onnxruntime.dylib"),
              let fingerprint = Bundle.main.url(forResource: "subject-runtime", withExtension: "sha256"), let modelURL else {
            throw AIClientError.message(L10n.text("本地推理运行库未打包，请重新构建应用。"))
        }
        let hash = try String(contentsOf: fingerprint, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        try engine.configureSubjectModel(runtimePath: library.path, runtimeHash: hash, modelPath: modelURL.path, backend: backend)
        engines.removeAll { $0.engine == nil }
        if !engines.contains(where: { $0.engine === engine }) { engines.append(SubjectEngineReference(engine)) }
    }
    private func installedStatus() { status = L10n.format("U²-Net-P · {0} · 已校验并缓存", String(describing: backend == "cpu" ? "CPU" : "Core ML")) }
    func selectBackend(_ value: String) {
        guard !downloading, ["cpu", "core_ml"].contains(value) else { return }
        backend = value; UserDefaults.standard.set(value, forKey: "subject.backend")
        do { for reference in engines { if let engine = reference.engine { try configure(engine) } }; if installed { installedStatus() }; error = "" }
        catch { self.error = error.localizedDescription }
    }
    func importModel(window: NSWindow, engine: StrataEngine) {
        guard !downloading else { return }
        let panel = NSOpenPanel(); panel.title = L10n.text("导入支持的 U²-Net-P 模型"); panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        panel.message = L10n.text("仅接受受支持版本及 SHA-256 完全匹配的模型；替换失败将保留当前缓存。")
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let source = panel.url else { return }
            self?.startImport(source, engine: engine)
        }
    }
    private func startImport(_ source: URL, engine: StrataEngine) {
        guard !downloading, let modelURL, let asset = try? SubjectModelAsset.current() else { return }
        downloading = true; progress = 0; error = ""; status = L10n.text("正在导入并校验模型…")
        download = Task { [weak self] in
            guard let self else { return }
            var temporary: URL?
            defer { self.downloading = false; self.download = nil; if let temporary { try? FileManager.default.removeItem(at: temporary) } }
            do {
                let staged = try await aiEngineWork { try SubjectModelInstallation.stage(source: source, destination: modelURL, maximumBytes: asset.maxBytes) }
                temporary = staged; try Task.checkCancellation()
                try SubjectModelInstallation.publish(staged, to: modelURL)
                self.installed = true; self.progress = 1; self.installedStatus(); try self.configure(engine)
            } catch { self.error = Task.isCancelled ? "" : error.localizedDescription; self.status = self.installed ? L10n.text("导入未完成；原缓存保留") : L10n.text("模型导入未完成") }
        }
    }
    func removeModel(window: NSWindow) {
        guard installed, !downloading, let modelURL else { return }
        let alert = NSAlert(); alert.messageText = L10n.text("移除主体模型缓存？")
        alert.informativeText = L10n.text("取消正在准备的本地任务并释放已加载的主体模型。图片、选区和历史会保留；以后可以重新下载或导入。")
        alert.addButton(withTitle: L10n.text("移除缓存")); alert.addButton(withTitle: L10n.text("取消"))
        alert.beginSheetModal(for: window) { [weak self] response in
            guard response == .alertFirstButtonReturn, let self else { return }
            do {
                for reference in self.engines { try reference.engine?.clearSubjectModel() }
                try FileManager.default.removeItem(at: modelURL)
                self.installed = false; self.status = L10n.text("主体模型缓存已移除"); self.error = ""
            } catch { self.error = error.localizedDescription }
        }
    }
    func ensureInstalled(window: NSWindow, engine: StrataEngine, completion: @escaping @MainActor () -> Void) {
        if installed {
            do { try configure(engine) } catch { self.error = error.localizedDescription; return }
            completion(); return
        }
        if downloading { waiters.append(completion); return }
        do {
            let asset = try SubjectModelAsset.current()
            let alert = NSAlert(); alert.messageText = L10n.text("下载本地主体选择模型？")
            alert.informativeText = L10n.format("模型：U²-Net-P（约 4.6 MB）\n许可：{0}\n来源：{1}\n下载：{2}\n\n图片只在本机推理。下载后校验 SHA-256；你可以取消或在离线时继续手工选框。", String(describing: asset.license), String(describing: asset.source), String(describing: asset.url))
            alert.addButton(withTitle: L10n.text("下载并选择主体")); alert.addButton(withTitle: L10n.text("取消"))
            alert.beginSheetModal(for: window) { [weak self] response in
                if response == .alertFirstButtonReturn { self?.startDownload(asset, engine: engine, completion: completion) }
            }
        } catch { self.error = error.localizedDescription }
    }
    private func startDownload(_ asset: SubjectModelAsset, engine: StrataEngine, completion: @escaping @MainActor () -> Void) {
        guard !downloading, let modelURL else { return }
        downloading = true; progress = 0; error = ""; status = L10n.text("正在下载 U²-Net-P…"); waiters.append(completion)
        download = Task { [weak self] in
            guard let self else { return }
            let temporary = modelURL.deletingLastPathComponent().appendingPathComponent("u2netp-\(UUID().uuidString).download")
            defer { self.downloading = false; self.download = nil; try? FileManager.default.removeItem(at: temporary) }
            do {
                try FileManager.default.createDirectory(at: temporary.deletingLastPathComponent(), withIntermediateDirectories: true)
                guard let url = URL(string: asset.url), url.scheme == "https" else { throw AIClientError.message(L10n.text("模型下载地址无效。")) }
                let configuration = URLSessionConfiguration.ephemeral
                configuration.urlCache = nil; configuration.httpCookieStorage = nil; configuration.urlCredentialStorage = nil
                configuration.timeoutIntervalForRequest = 30; configuration.timeoutIntervalForResource = 120
                let session = URLSession(configuration: configuration); defer { session.invalidateAndCancel() }
                let (bytes, response) = try await session.bytes(from: url)
                guard let response = response as? HTTPURLResponse, response.statusCode == 200, response.url?.scheme == "https" else { throw AIClientError.message(L10n.text("模型下载失败，请检查网络后重试。")) }
                guard FileManager.default.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw AIClientError.message(L10n.text("无法创建模型下载文件。")) }
                let file = try FileHandle(forWritingTo: temporary); defer { try? file.close() }
                var buffer = Data(); var received: UInt64 = 0
                for try await byte in bytes {
                    try Task.checkCancellation(); buffer.append(byte); received += 1
                    if received > asset.maxBytes { throw AIClientError.message(L10n.text("模型下载超过大小限制，已取消。")) }
                    if buffer.count >= 65536 {
                        try file.write(contentsOf: buffer); buffer.removeAll(keepingCapacity: true)
                        self.progress = response.expectedContentLength > 0 ? min(1, Double(received) / Double(response.expectedContentLength)) : 0
                    }
                }
                if !buffer.isEmpty { try file.write(contentsOf: buffer) }
                try file.synchronize(); try Task.checkCancellation()
                try await aiEngineWork { try verifySubjectModel(path: temporary.path) }
                try Task.checkCancellation()
                try SubjectModelInstallation.publish(temporary, to: modelURL)
                self.installed = true; self.progress = 1; self.installedStatus()
                try self.configure(engine)
                let callbacks = self.waiters; self.waiters = []; callbacks.forEach { $0() }
            } catch is CancellationError { self.status = L10n.text("模型下载已取消"); self.waiters = [] }
            catch { self.status = Task.isCancelled ? L10n.text("模型下载已取消") : L10n.text("模型下载失败"); self.error = Task.isCancelled ? "" : error.localizedDescription; self.waiters = [] }
        }
    }
    func cancelDownload() { download?.cancel() }
}
