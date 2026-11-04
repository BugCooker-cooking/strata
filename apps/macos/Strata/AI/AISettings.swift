import AppKit
import CryptoKit
import Foundation
import Security

enum AIProvider: String, Codable, CaseIterable, Sendable {
    case anthropic, openAi = "open_ai", local
    var title: String { switch self { case .anthropic: "Anthropic"; case .openAi: "OpenAI 兼容"; case .local: "本地模型" } }
    var defaultEndpoint: String { switch self { case .anthropic: "https://api.anthropic.com/v1"; case .openAi: "https://api.openai.com/v1"; case .local: "http://127.0.0.1:11434/v1" } }
}

struct AIConfiguration: Codable, Equatable, Identifiable, Sendable {
    var id = UUID()
    var kind = AIProvider.openAi
    var endpoint = AIProvider.openAi.defaultEndpoint
    var model = ""
    var vision = false
    var toolsVerified = false
    var allowInsecureHTTP = false
    var title: String { "\(kind.title) · \(model)" }
    func json() throws -> String {
        try commandJSON(["kind": kind.rawValue, "endpoint": endpoint, "model": model, "vision": vision, "allow_insecure_http": allowInsecureHTTP])
    }
    func validated() throws -> AIJSON { try AIJSON.decode(aiValidateConfig(configJson: json())) }
    func secretAccount() throws -> String {
        let identity = try validated()["identity"].string ?? ""
        return SHA256.hash(data: Data(identity.utf8)).map { String(format: "%02x", $0) }.joined()
    }
}

protocol AISecretStore: Sendable {
    func read(account: String) throws -> String?
    func save(_ value: String, account: String) throws
    func remove(account: String) throws
}

struct KeychainAISecrets: AISecretStore {
    private let service = "dev.strata-editor.ai"
    private func query(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account]
    }
    func read(account: String) throws -> String? {
        var request = query(account); request[kSecReturnData as String] = true; request[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(request as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data, let value = String(data: data, encoding: .utf8) else { throw AIClientError.message("无法从钥匙串读取密钥（\(status)）。") }
        return value
    }
    func save(_ value: String, account: String) throws {
        guard !value.contains("\n"), !value.contains("\r"), value.utf8.count <= 16384 else { throw AIClientError.message("密钥格式无效。") }
        let attributes = [kSecValueData as String: Data(value.utf8)]
        var status = SecItemUpdate(query(account) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var request = query(account); request.merge(attributes) { _, new in new }
            request[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            status = SecItemAdd(request as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw AIClientError.message("无法保存到钥匙串（\(status)）。") }
    }
    func remove(account: String) throws {
        let status = SecItemDelete(query(account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw AIClientError.message("无法删除钥匙串密钥（\(status)）。") }
    }
}

@MainActor
final class AISettingsStore: ObservableObject {
    static let shared = AISettingsStore()
    @Published private(set) var configurations: [AIConfiguration]
    @Published var selected: UUID? { didSet { defaults.set(selected?.uuidString, forKey: "ai.selected") } }
    let secrets: any AISecretStore
    private let defaults: UserDefaults
    init(defaults: UserDefaults = .standard, secrets: any AISecretStore = KeychainAISecrets()) {
        self.defaults = defaults; self.secrets = secrets
        configurations = defaults.data(forKey: "ai.configurations").flatMap { try? JSONDecoder().decode([AIConfiguration].self, from: $0) } ?? []
        selected = defaults.string(forKey: "ai.selected").flatMap(UUID.init(uuidString:)) ?? configurations.first?.id
    }
    var current: AIConfiguration? { configurations.first { $0.id == selected } }
    func credential(for config: AIConfiguration, replacement: String = "") throws -> String {
        if !replacement.isEmpty { return replacement }
        let value = try secrets.read(account: config.secretAccount()) ?? ""
        if config.kind != .local && value.isEmpty { throw AIClientError.message("请为这个服务地址与模型录入 API Key。") }
        return value
    }
    func save(_ config: AIConfiguration, key: String) throws {
        _ = try config.validated()
        if !key.isEmpty { try secrets.save(key, account: config.secretAccount()) }
        var next = configurations.filter { $0.id != config.id }; next.append(config)
        let data = try JSONEncoder().encode(next)
        defaults.set(data, forKey: "ai.configurations"); configurations = next; selected = config.id
    }
    func remove(_ config: AIConfiguration) throws {
        try secrets.remove(account: config.secretAccount())
        configurations.removeAll { $0.id == config.id }
        defaults.set(try JSONEncoder().encode(configurations), forKey: "ai.configurations")
        selected = configurations.first?.id
    }
}

enum AIClientError: LocalizedError, Sendable {
    case message(String), http(Int)
    var errorDescription: String? {
        switch self { case .message(let text): text; case .http(let code): "模型服务返回 HTTP \(code)。请检查地址、模型、权限或稍后重试。" }
    }
    var retryable: Bool { if case .http(let code) = self { return code == 408 || code == 429 || code >= 500 }; return false }
}

/// Foundation-independent JSON values stay Sendable across the UI/engine queues.
enum AIJSON: Codable, Sendable, Equatable {
    case object([String: AIJSON]), array([AIJSON]), string(String), number(Double), bool(Bool), null
    init(from decoder: Decoder) throws {
        let value = try decoder.singleValueContainer()
        if value.decodeNil() { self = .null }
        else if let result = try? value.decode(Bool.self) { self = .bool(result) }
        else if let result = try? value.decode(String.self) { self = .string(result) }
        else if let result = try? value.decode(Double.self) { self = .number(result) }
        else if let result = try? value.decode([AIJSON].self) { self = .array(result) }
        else { self = .object(try value.decode([String: AIJSON].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .object(let item): try value.encode(item)
        case .array(let item): try value.encode(item)
        case .string(let item): try value.encode(item)
        case .number(let item): try value.encode(item)
        case .bool(let item): try value.encode(item)
        case .null: try value.encodeNil()
        }
    }
    subscript(_ key: String) -> AIJSON { if case .object(let value) = self { return value[key] ?? .null }; return .null }
    var string: String? { if case .string(let value) = self { return value }; return nil }
    var array: [AIJSON] { if case .array(let value) = self { return value }; return [] }
    var number: Double? { if case .number(let value) = self { return value }; return nil }
    var bool: Bool { if case .bool(let value) = self { return value }; return false }
    func encoded() throws -> String { String(decoding: try JSONEncoder().encode(self), as: UTF8.self) }
    static func decode(_ text: String) throws -> AIJSON { try JSONDecoder().decode(Self.self, from: Data(text.utf8)) }
}
