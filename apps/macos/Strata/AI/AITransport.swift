import Foundation

struct AIWireRequest: Decodable, Sendable {
    let url: String
    let provider: AIProvider
    let method: String
    let body: AIJSON
    static func decode(_ text: String) throws -> Self { try JSONDecoder().decode(Self.self, from: Data(text.utf8)) }
}

/// A credential-bearing request never follows a redirect. URLSession's normal
/// TLS validation stays enabled; cookies, credentials and response caching are off.
private final class AITransportDelegate: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) { completionHandler(nil) }
}

struct AITransport: Sendable {
    func send(_ wire: AIWireRequest, key: String, timeout: TimeInterval = 60,
              onChunk: @escaping @Sendable (Data) async throws -> Void) async throws {
        guard let url = URL(string: wire.url), ["https", "http"].contains(url.scheme ?? ""), ["POST", "GET"].contains(wire.method),
              !key.contains("\r"), !key.contains("\n") else { throw AIClientError.message(L10n.text("服务地址或密钥格式无效。")) }
        let config = URLSessionConfiguration.ephemeral
        config.httpCookieStorage = nil; config.urlCredentialStorage = nil; config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.timeoutIntervalForRequest = timeout; config.timeoutIntervalForResource = timeout
        if wire.provider == .local { config.connectionProxyDictionary = [:] }
        let session = URLSession(configuration: config, delegate: AITransportDelegate(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        var request = URLRequest(url: url); request.httpMethod = wire.method
        if wire.method == "POST" { request.httpBody = Data(try wire.body.encoded().utf8) }
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        if !key.isEmpty {
            request.setValue(wire.provider == .anthropic ? key : "Bearer \(key)", forHTTPHeaderField: wire.provider == .anthropic ? "x-api-key" : "Authorization")
        }
        if wire.provider == .anthropic { request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version") }
        try await withTaskCancellationHandler {
            let (bytes, response) = try await session.bytes(for: request)
            guard let response = response as? HTTPURLResponse, response.url == url else { throw AIClientError.message(L10n.text("模型服务响应地址不匹配。")) }
            guard (200..<300).contains(response.statusCode) else { throw AIClientError.http(response.statusCode) }
            var buffer = Data(); var total = 0
            for try await byte in bytes {
                try Task.checkCancellation(); buffer.append(byte); total += 1
                guard total <= 4 * 1024 * 1024 else { throw AIClientError.message(L10n.text("模型回复超过 4 MiB 限制，已停止。")) }
                if byte == 10 || buffer.count >= 8192 { try await onChunk(buffer); buffer.removeAll(keepingCapacity: true) }
            }
            if !buffer.isEmpty { try await onChunk(buffer) }
        } onCancel: { session.invalidateAndCancel() }
    }
    func collect(_ wire: AIWireRequest, key: String) async throws -> Data {
        let collector = AIResponseCollector()
        try await send(wire, key: key) { chunk in await collector.append(chunk) }
        return await collector.data
    }
}

private actor AIResponseCollector {
    var data = Data()
    func append(_ chunk: Data) { data.append(chunk) }
}

func aiEngineWork<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await Task.detached(priority: .userInitiated, operation: work).value
}
