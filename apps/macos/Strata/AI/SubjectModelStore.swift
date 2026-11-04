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

@MainActor
final class SubjectModelStore: ObservableObject {
    static let shared = SubjectModelStore()
    @Published private(set) var installed = false
    @Published private(set) var downloading = false
    @Published private(set) var progress = 0.0
    @Published private(set) var status = "主体模型尚未下载"
    @Published var error = ""
    private var download: Task<Void, Never>?
    private var waiters: [@MainActor () -> Void] = []
    private(set) var modelURL: URL?
    init() {
        do {
            let root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            modelURL = root.appendingPathComponent("Strata/Models/u2netp-rembg-v0.0.0.onnx")
            if let modelURL, (try? verifySubjectModel(path: modelURL.path)) != nil { installed = true; status = "U²-Net-P · CPU · 已安装" }
        } catch { self.error = error.localizedDescription }
    }
    func configure(_ engine: StrataEngine) throws {
        guard let library = Bundle.main.privateFrameworksURL?.appendingPathComponent("libstrata_onnxruntime.dylib"),
              let fingerprint = Bundle.main.url(forResource: "subject-runtime", withExtension: "sha256"), let modelURL else {
            throw AIClientError.message("本地推理运行库未打包，请重新构建应用。")
        }
        let hash = try String(contentsOf: fingerprint, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        try engine.configureSubjectModel(runtimePath: library.path, runtimeHash: hash, modelPath: modelURL.path, backend: "cpu")
    }
    func ensureInstalled(window: NSWindow, engine: StrataEngine, completion: @escaping @MainActor () -> Void) {
        if installed {
            do { try configure(engine) } catch { self.error = error.localizedDescription; return }
            completion(); return
        }
        if downloading { waiters.append(completion); return }
        do {
            let asset = try SubjectModelAsset.current()
            let alert = NSAlert(); alert.messageText = "下载本地主体选择模型？"
            alert.informativeText = "模型：U²-Net-P（约 4.6 MB）\n许可：\(asset.license)\n来源：\(asset.source)\n下载：\(asset.url)\n\n图片只在本机推理。下载后校验 SHA-256；你可以取消或在离线时继续手工选框。"
            alert.addButton(withTitle: "下载并选择主体"); alert.addButton(withTitle: "取消")
            alert.beginSheetModal(for: window) { [weak self] response in
                if response == .alertFirstButtonReturn { self?.startDownload(asset, engine: engine, completion: completion) }
            }
        } catch { self.error = error.localizedDescription }
    }
    private func startDownload(_ asset: SubjectModelAsset, engine: StrataEngine, completion: @escaping @MainActor () -> Void) {
        guard !downloading, let modelURL else { return }
        downloading = true; progress = 0; error = ""; status = "正在下载 U²-Net-P…"; waiters.append(completion)
        download = Task { [weak self] in
            guard let self else { return }
            let temporary = modelURL.deletingLastPathComponent().appendingPathComponent("u2netp-\(UUID().uuidString).download")
            defer { self.downloading = false; self.download = nil; try? FileManager.default.removeItem(at: temporary) }
            do {
                try FileManager.default.createDirectory(at: temporary.deletingLastPathComponent(), withIntermediateDirectories: true)
                guard let url = URL(string: asset.url), url.scheme == "https" else { throw AIClientError.message("模型下载地址无效。") }
                let configuration = URLSessionConfiguration.ephemeral
                configuration.urlCache = nil; configuration.httpCookieStorage = nil; configuration.urlCredentialStorage = nil
                configuration.timeoutIntervalForRequest = 30; configuration.timeoutIntervalForResource = 120
                let session = URLSession(configuration: configuration); defer { session.invalidateAndCancel() }
                let (bytes, response) = try await session.bytes(from: url)
                guard let response = response as? HTTPURLResponse, response.statusCode == 200, response.url?.scheme == "https" else { throw AIClientError.message("模型下载失败，请检查网络后重试。") }
                guard FileManager.default.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw AIClientError.message("无法创建模型下载文件。") }
                let file = try FileHandle(forWritingTo: temporary); defer { try? file.close() }
                var buffer = Data(); var received: UInt64 = 0
                for try await byte in bytes {
                    try Task.checkCancellation(); buffer.append(byte); received += 1
                    if received > asset.maxBytes { throw AIClientError.message("模型下载超过大小限制，已取消。") }
                    if buffer.count >= 65536 {
                        try file.write(contentsOf: buffer); buffer.removeAll(keepingCapacity: true)
                        self.progress = response.expectedContentLength > 0 ? min(1, Double(received) / Double(response.expectedContentLength)) : 0
                    }
                }
                if !buffer.isEmpty { try file.write(contentsOf: buffer) }
                try file.synchronize(); try Task.checkCancellation()
                try await aiEngineWork { try verifySubjectModel(path: temporary.path) }
                try Task.checkCancellation()
                if FileManager.default.fileExists(atPath: modelURL.path) { _ = try FileManager.default.replaceItemAt(modelURL, withItemAt: temporary) }
                else { try FileManager.default.moveItem(at: temporary, to: modelURL) }
                self.installed = true; self.progress = 1; self.status = "U²-Net-P · CPU · 已安装"
                try self.configure(engine)
                let callbacks = self.waiters; self.waiters = []; callbacks.forEach { $0() }
            } catch is CancellationError { self.status = "模型下载已取消"; self.waiters = [] }
            catch { self.status = Task.isCancelled ? "模型下载已取消" : "模型下载失败"; self.error = Task.isCancelled ? "" : error.localizedDescription; self.waiters = [] }
        }
    }
    func cancelDownload() { download?.cancel() }
}
