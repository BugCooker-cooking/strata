import Combine
import Foundation

struct AiTurnPreferences: Codable, Equatable, Sendable {
    var mode = "ask_first"
    var maxSteps: UInt64 = 50
    var requestTimeoutSeconds: UInt64 = 60
    var turnTimeoutSeconds: UInt64 = 300
}

struct MemoryBudgetPreferences: Codable, Equatable, Sendable {
    var cpuMib: UInt64 = 2048
    var gpuMib: UInt64 = 512
    var diskMib: UInt64 = 32768
}

struct ApplicationPreferencesSnapshot: Decodable, Equatable, Sendable {
    var version: UInt64 = 0
    var ai = AiTurnPreferences()
    var memory = MemoryBudgetPreferences()
}

@MainActor
final class ApplicationPreferences: ObservableObject {
    static let shared = ApplicationPreferences()
    static let key = "application.preferences.v1"
    @Published private(set) var snapshot = ApplicationPreferencesSnapshot()
    @Published private(set) var restoreError = ""
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    func read(from engine: StrataEngine) throws -> ApplicationPreferencesSnapshot {
        let json = try engine.dispatch(command: "system.preferences.get", paramsJson: "{}")
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(ApplicationPreferencesSnapshot.self, from: Data(json.utf8))
    }

    func restore(on engine: StrataEngine) throws {
        do {
            if let saved = defaults.string(forKey: Self.key) {
                guard saved.utf8.count <= 8192,
                      var options = try JSONSerialization.jsonObject(with: Data(saved.utf8)) as? [String: Any],
                      Set(options.keys) == ["ai", "memory"] else {
                    throw QuickExportError("应用偏好数据无效，请重新保存偏好设置。")
                }
                options["expected_version"] = try read(from: engine).version
                _ = try engine.dispatch(command: "system.preferences.set", paramsJson: commandJSON(options))
            }
            snapshot = try read(from: engine)
            restoreError = ""
        } catch {
            restoreError = "应用偏好未能恢复，请重新保存偏好设置。\(error.localizedDescription)"
            throw error
        }
    }

    func save(from engine: StrataEngine) throws {
        let json = try engine.dispatch(command: "system.preferences.get", paramsJson: "{}")
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let current = try decoder.decode(ApplicationPreferencesSnapshot.self, from: Data(json.utf8))
        guard var values = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any] else {
            throw QuickExportError("无法读取应用偏好。")
        }
        values.removeValue(forKey: "version")
        let encoded = try JSONSerialization.data(withJSONObject: values, options: [.sortedKeys])
        defaults.set(String(decoding: encoded, as: UTF8.self), forKey: Self.key)
        snapshot = current
        restoreError = ""
    }

    func sessionOptions(engine: StrataEngine, askFirst: Bool) throws -> String {
        guard restoreError.isEmpty else { throw QuickExportError(restoreError) }
        var options = try read(from: engine).ai
        options.mode = askFirst ? "ask_first" : "direct"
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return String(decoding: try encoder.encode(options), as: UTF8.self)
    }
}
