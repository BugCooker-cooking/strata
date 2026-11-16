import Foundation

struct AIChatMessage: Codable, Identifiable, Sendable {
    var id = UUID()
    let role: String
    var text: String
    var turn: String?
}
struct AIConversationSummary: Identifiable, Sendable {
    let id: UUID; let title: String; let created: Date; let current: Bool
}
private struct AIConversation: Codable {
    var id = UUID()
    var created = Date()
    var messages: [AIChatMessage] = []
}
private struct AIConversationArchive: Codable {
    var version = 2
    var current: UUID
    var conversations: [AIConversation]
    init() { let first=AIConversation();current=first.id;conversations=[first] }
}

struct AIConversationStore: Sendable {
    let directory: URL
    static func applicationStore() throws -> Self {
        let root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        return Self(directory: root.appendingPathComponent("Strata/Conversations", isDirectory: true))
    }
    private func url(_ document: String) throws -> URL {
        guard let uuid = UUID(uuidString: document) else { throw AIClientError.message(L10n.text("无效的对话文档标识。")) }
        return directory.appendingPathComponent(uuid.uuidString + ".json")
    }
    private func archive(document: String) throws -> AIConversationArchive {
        let path = try url(document)
        if !FileManager.default.fileExists(atPath: path.path) { return AIConversationArchive() }
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        guard (attributes[.size] as? NSNumber)?.intValue ?? Int.max <= 2 * 1024 * 1024 else { throw AIClientError.message(L10n.text("对话记录超过大小限制。")) }
        let data=try Data(contentsOf:path)
        let result:AIConversationArchive
        if let old=try? JSONDecoder().decode([AIChatMessage].self,from:data) {
            var migrated=AIConversationArchive()
            let legacy=AIConversation(id:UUID(uuidString:document) ?? migrated.current,created:(attributes[.modificationDate] as? Date) ?? Date(),messages:old)
            migrated.current=legacy.id;migrated.conversations=[legacy];result=migrated
        } else {result=try JSONDecoder().decode(AIConversationArchive.self,from:data)}
        guard result.version==2, !result.conversations.isEmpty,result.conversations.count<=30,
              Set(result.conversations.map(\.id)).count==result.conversations.count,
              result.conversations.contains(where:{$0.id==result.current}),
              result.conversations.allSatisfy({conversation in conversation.messages.count<=500 && conversation.messages.allSatisfy{["user","assistant","system"].contains($0.role)&&$0.text.utf8.count<=256*1024}})
        else {throw AIClientError.message(L10n.text("对话记录格式或版本无效。"))}
        return result
    }
    func load(document:String)throws->[AIChatMessage] {
        let archive=try archive(document:document)
        return archive.conversations.first{$0.id==archive.current}?.messages ?? []
    }
    func list(document:String)throws->[AIConversationSummary]{
        if !FileManager.default.fileExists(atPath:try url(document).path){return []}
        let archive=try archive(document:document)
        return archive.conversations.reversed().map{conversation in
            AIConversationSummary(id:conversation.id,title:String((conversation.messages.first{$0.role=="user"}?.text ?? L10n.text("新对话")).prefix(40)),created:conversation.created,current:conversation.id==archive.current)
        }
    }
    func newConversation(document:String)throws{
        var archive=try archive(document:document)
        guard archive.conversations.count<30 else{throw AIClientError.message(L10n.text("此文档已有 30 段对话，请先保留或管理历史后再新建。"))}
        let next=AIConversation();archive.current=next.id;archive.conversations.append(next);try write(archive,document:document)
    }
    func activate(_ id:UUID,document:String)throws{
        var archive=try archive(document:document)
        guard archive.conversations.contains(where:{$0.id==id}) else{throw AIClientError.message(L10n.text("找不到这段对话。"))}
        archive.current=id;try write(archive,document:document)
    }
    func save(_ messages: [AIChatMessage], document: String) throws {
        guard messages.count<=500 else{throw AIClientError.message(L10n.text("此段对话已达存储上限，请新建对话继续。"))}
        var archive=try archive(document:document)
        guard let index=archive.conversations.firstIndex(where:{$0.id==archive.current}) else{throw AIClientError.message(L10n.text("找不到当前对话。"))}
        archive.conversations[index].messages=messages;try write(archive,document:document)
    }
    private func write(_ archive:AIConversationArchive,document:String)throws{
        let path = try url(document)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let data = try JSONEncoder().encode(archive)
        guard data.count <= 2 * 1024 * 1024 else { throw AIClientError.message(L10n.text("对话记录超过大小限制。")) }
        try data.write(to: path, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
    }
}
