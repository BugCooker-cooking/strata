import Foundation
import XCTest
@testable import Strata

private final class MemoryAISecrets: AISecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var entries: [String: String] = [:]
    func read(account: String) throws -> String? { lock.withLock { entries[account] } }
    func save(_ value: String, account: String) throws { lock.withLock { entries[account] = value } }
    func remove(account: String) throws { _ = lock.withLock { entries.removeValue(forKey: account) } }
}

final class AISettingsTests: XCTestCase {
    @MainActor
    func testKeysAreIsolatedByDestinationModelAndProviderAndNeverStoredInPreferences() throws {
        let suite = "Strata.AISettingsTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite)); defer { defaults.removePersistentDomain(forName: suite) }
        let secrets = MemoryAISecrets(); let store = AISettingsStore(defaults: defaults, secrets: secrets)
        var config = AIConfiguration(); config.endpoint = "https://example.test/v1"; config.model = "model-a"; config.toolsVerified = true
        let sentinel = "test-only-secret-\(UUID().uuidString)"
        try store.save(config, key: sentinel)
        XCTAssertEqual(try store.credential(for: config), sentinel)
        let preferences = try XCTUnwrap(defaults.data(forKey: "ai.configurations"))
        XCTAssertFalse(String(decoding: preferences, as: UTF8.self).contains(sentinel))
        var changed = config; changed.endpoint = "https://other.test/v1"
        XCTAssertThrowsError(try store.credential(for: changed))
        changed = config; changed.model = "model-b"; XCTAssertThrowsError(try store.credential(for: changed))
        changed = config; changed.kind = .anthropic; XCTAssertThrowsError(try store.credential(for: changed))
        XCTAssertEqual(AISettingsStore(defaults: defaults, secrets: secrets).current, config)
        try store.remove(config); XCTAssertTrue(store.configurations.isEmpty); XCTAssertThrowsError(try store.credential(for: config))
    }

    func testRemoteHttpRequiresExplicitOptInAndLocalProviderStaysOnLoopback() throws {
        var config = AIConfiguration(); config.endpoint = "http://example.test:8080/v1"; config.model = "test"
        XCTAssertThrowsError(try config.validated())
        config.allowInsecureHTTP = true
        XCTAssertEqual(try config.validated()["url"].string, "http://example.test:8080/v1/chat/completions")
        config.kind = .local; XCTAssertThrowsError(try config.validated())
        config.endpoint = "http://127.0.0.1:11434/v1"; config.allowInsecureHTTP = false
        XCTAssertNoThrow(try config.validated())
        config.endpoint = "https://user:password@example.test/v1"; XCTAssertThrowsError(try config.validated())
    }

    func testDocumentConversationsAreIndependentAtomicAndContainNoImagesOrCredentials() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Strata-AI-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let store = AIConversationStore(directory: root)
        let first = UUID().uuidString; let second = UUID().uuidString
        let messages = [AIChatMessage(role: "user", text: "新建草稿"), AIChatMessage(role: "assistant", text: "已创建", turn: UUID().uuidString)]
        try store.save(messages, document: first)
        XCTAssertEqual(try store.load(document: first).map(\.text), messages.map(\.text))
        XCTAssertTrue(try store.load(document: second).isEmpty)
        XCTAssertThrowsError(try store.save(messages, document: "../../escape"))
        let tooLarge = [AIChatMessage(role: "user", text: String(repeating: "x", count: 3 * 1024 * 1024))]
        XCTAssertThrowsError(try store.save(tooLarge, document: first))
        XCTAssertEqual(try store.load(document: first).map(\.text), messages.map(\.text))
        try store.save([], document: first); XCTAssertTrue(try store.load(document: first).isEmpty)
    }

    @MainActor
    func testNativeAssistantBindsDocumentWithoutNetworkingAndRestoresUserControlAfterStop() throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 2, "height": 2, "bit_depth": 16]))
        let assistant = AssistantModel(engine: engine)
        let state = try EditorSnapshot.decode(engine.snapshotJson())
        assistant.bind(document: state.documentId)
        XCTAssertFalse(assistant.running)
        assistant.bind(document: UUID().uuidString)
        XCTAssertTrue(assistant.messages.isEmpty)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).control?.kind, "user")
    }
}
