import AppKit
import XCTest
@testable import Strata

@MainActor
final class ApplicationPreferencesTests: XCTestCase {
    private func engine() -> StrataEngine { StrataEngine(listener: EngineEvents { _ in }) }

    private func save(_ engine: StrataEngine, ai: [String: Any]) throws {
        _ = try engine.dispatch(command: "system.preferences.set", paramsJson: commandJSON(["ai": ai]))
    }

    private func restore(_ engine: StrataEngine, _ original: ApplicationPreferencesSnapshot) throws {
        _ = try engine.dispatch(command: "system.preferences.set", paramsJson: commandJSON([
            "ai": ["mode": original.ai.mode, "max_steps": original.ai.maxSteps,
                   "request_timeout_seconds": original.ai.requestTimeoutSeconds, "turn_timeout_seconds": original.ai.turnTimeoutSeconds],
            "memory": ["cpu_mib": original.memory.cpuMib, "gpu_mib": original.memory.gpuMib, "disk_mib": original.memory.diskMib]
        ]))
    }

    func testPersistenceRestoresValidatedGroupsAndRejectsCorruptOrUnboundedData() throws {
        let suite = "Strata.ApplicationPreferencesTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        let preferences = ApplicationPreferences(defaults: defaults)
        let a = engine(), b = engine()
        let original = try preferences.read(from: a)
        defer { try? restore(a, original); defaults.removePersistentDomain(forName: suite) }
        try save(a, ai: ["mode":"direct","max_steps":9,"request_timeout_seconds":13,"turn_timeout_seconds":45])
        try preferences.save(from: a)
        let expected = try preferences.read(from: a)
        XCTAssertEqual(try preferences.read(from: b), expected)
        let stored = try XCTUnwrap(defaults.string(forKey: ApplicationPreferences.key))
        XCTAssertFalse(stored.contains("resource"))
        XCTAssertFalse(stored.contains("version"))
        try restore(a, original)
        try preferences.restore(on: b)
        XCTAssertEqual(try preferences.read(from: b).ai, expected.ai)
        XCTAssertEqual(try preferences.read(from: b).memory, expected.memory)

        defaults.set("{\"ai\":{\"mode\":\"direct\",\"max_steps\":999,\"request_timeout_seconds\":0,\"turn_timeout_seconds\":0},\"memory\":{\"cpu_mib\":0,\"gpu_mib\":0,\"disk_mib\":0}}", forKey: ApplicationPreferences.key)
        let before = try preferences.read(from: b)
        XCTAssertThrowsError(try preferences.restore(on: b))
        XCTAssertEqual(try preferences.read(from: b), before)
        XCTAssertFalse(preferences.restoreError.isEmpty)
        XCTAssertThrowsError(try preferences.sessionOptions(engine: b, askFirst: true))
        defaults.set(stored, forKey: ApplicationPreferences.key)
        try preferences.restore(on: b)
        XCTAssertTrue(preferences.restoreError.isEmpty)
    }

    func testActualTurnOptionsUseSavedLimitsAndPreservePerTurnModeChoice() throws {
        let suite = "Strata.ApplicationTurnDefaults.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        let preferences = ApplicationPreferences(defaults: defaults)
        let e = engine()
        let original = try preferences.read(from: e)
        defer { try? restore(e, original); defaults.removePersistentDomain(forName: suite) }
        try save(e, ai: ["mode":"direct","max_steps":9,"request_timeout_seconds":13,"turn_timeout_seconds":45])
        try preferences.save(from: e)
        let assistant = AssistantModel(engine: e, preferences: preferences)
        XCTAssertFalse(assistant.askFirst)
        let json = try AIJSON.decode(preferences.sessionOptions(engine: e, askFirst: true))
        XCTAssertEqual(json["mode"].string, "ask_first")
        XCTAssertEqual(json["max_steps"].number, 9)
        XCTAssertEqual(json["request_timeout_seconds"].number, 13)
        XCTAssertEqual(json["turn_timeout_seconds"].number, 45)
        try save(e, ai: ["mode":"ask_first","max_steps":20,"request_timeout_seconds":30,"turn_timeout_seconds":120])
        try preferences.save(from: e)
        XCTAssertTrue(assistant.askFirst)
    }

    func testPreferencesFormUsesCurrentVersionAndInvalidSaveKeepsTheSheetAvailable() async throws {
        let e = engine()
        _ = try e.dispatch(command: "file.new", paramsJson: commandJSON(["width":8,"height":8,"bit_depth":16]))
        let options = try PreferencesOptionsController(engine: e)
        let state = try e.snapshotJson()
        let original = try ApplicationPreferences.shared.read(from: e)
        options.maxSteps.stringValue = "0"
        options.apply()
        for _ in 0..<100 {
            if !options.running { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertFalse(options.running)
        XCTAssertTrue(options.status.stringValue.contains("AI 步数"))
        XCTAssertFalse(options.status.stringValue.contains("图层"))
        XCTAssertTrue(options.applyButton.isEnabled)
        XCTAssertEqual(try ApplicationPreferences.shared.read(from: e), original)
        XCTAssertEqual(try e.snapshotJson(), state)
        options.maxSteps.stringValue = "12"
        options.cpu.stringValue = "255"
        options.apply()
        for _ in 0..<100 {
            if !options.running { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertFalse(options.running)
        XCTAssertTrue(options.status.stringValue.contains("内存额度"))
        XCTAssertEqual(try ApplicationPreferences.shared.read(from: e), original)
        XCTAssertEqual(try e.snapshotJson(), state)
        options.cpu.stringValue = String(original.memory.cpuMib)
        let params = try options.parameters()
        XCTAssertEqual(params["expected_version"] as? UInt64, original.version)
        XCTAssertEqual((params["ai"] as? [String: Any])?["max_steps"] as? UInt64, 12)
        XCTAssertTrue(options.usage.stringValue.contains("CPU"))
    }

    func testStaleFormExplainsHowToReloadAndPreservesTheNewerSettings() async throws {
        let e = engine(), otherTab = engine()
        let original = try ApplicationPreferences.shared.read(from: e)
        defer { try? restore(e, original) }
        let options = try PreferencesOptionsController(engine: e)
        let updatedSteps: UInt64 = original.ai.maxSteps == 12 ? 13 : 12
        try save(otherTab, ai: ["mode":"ask_first", "max_steps":updatedSteps,
                                "request_timeout_seconds":15, "turn_timeout_seconds":90])
        let newer = try ApplicationPreferences.shared.read(from: e)
        options.apply()
        for _ in 0..<100 {
            if !options.running { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertFalse(options.running)
        XCTAssertTrue(options.applyButton.isEnabled)
        XCTAssertTrue(options.status.stringValue.contains("关闭后重新打开"))
        XCTAssertEqual(try ApplicationPreferences.shared.read(from: e), newer)
    }
}
