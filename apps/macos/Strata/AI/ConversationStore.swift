import Foundation

struct AIChatMessage: Codable, Identifiable, Sendable {
    var id = UUID()
    let role: String
    var text: String
    var turn: String?
}

struct AIConversationStore: Sendable {
    let directory: URL
    static func applicationStore() throws -> Self {
        let root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        return Self(directory: root.appendingPathComponent("Strata/Conversations", isDirectory: true))
    }
    private func url(_ document: String) throws -> URL {
        guard let uuid = UUID(uuidString: document) else { throw AIClientError.message("无效的对话文档标识。") }
        return directory.appendingPathComponent(uuid.uuidString + ".json")
    }
    func load(document: String) throws -> [AIChatMessage] {
        let path = try url(document)
        if !FileManager.default.fileExists(atPath: path.path) { return [] }
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        guard (attributes[.size] as? NSNumber)?.intValue ?? Int.max <= 2 * 1024 * 1024 else { throw AIClientError.message("对话记录超过大小限制。") }
        let messages = try JSONDecoder().decode([AIChatMessage].self, from: Data(contentsOf: path))
        guard messages.count <= 500, messages.allSatisfy({ ["user", "assistant", "system"].contains($0.role) && $0.text.utf8.count <= 256 * 1024 }) else { throw AIClientError.message("对话记录格式无效。") }
        return messages
    }
    func save(_ messages: [AIChatMessage], document: String) throws {
        let path = try url(document)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let data = try JSONEncoder().encode(Array(messages.suffix(500)))
        guard data.count <= 2 * 1024 * 1024 else { throw AIClientError.message("对话记录超过大小限制。") }
        try data.write(to: path, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
    }
}
