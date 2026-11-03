import XCTest
@testable import Strata

final class RecoveryTests: XCTestCase {
    @MainActor
    func testRealIdleMonitorWritesCheckpointAndSuccessfulSaveCleansIt() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let engine = StrataEngine(listener: EngineEvents { _ in })
        let controller = MainWindowController(engine: engine)
        controller.startRecovery(directory: directory.appendingPathComponent("recovery"))
        defer { controller.recovery?.stop(discard: false) }
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 2, "height": 2, "bit_depth": 16]))
        controller.refresh()
        XCTAssertTrue(try RecoveryEntry.decode(engine.recoveryEntriesJson()).isEmpty)
        try await Task.sleep(for: .seconds(32))
        let entries = try RecoveryEntry.decode(engine.recoveryEntriesJson())
        XCTAssertEqual(entries.count, 1)
        XCTAssertTrue(try EditorSnapshot.decode(engine.snapshotJson()).dirty)
        XCTAssertTrue(controller.workspace.recoveryStatus.hasPrefix("恢复副本"))
        let target = try engine.grantWrite(path: directory.appendingPathComponent("saved.strata").path, overwrite: false)
        _ = try engine.dispatch(command: "file.save_as", paramsJson: commandJSON(["resource": target]))
        controller.refresh()
        for _ in 0..<20 {
            if try RecoveryEntry.decode(engine.recoveryEntriesJson()).isEmpty { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTAssertTrue(try RecoveryEntry.decode(engine.recoveryEntriesJson()).isEmpty)
    }
    func testIdleDeadlineBusyDeferralAndEditsDuringCheckpoint() {
        var state = RecoverySchedule()
        state.observe(revision: 1, dirty: true, now: 100)
        XCTAssertNil(state.begin(now: 129.9, busy: false))
        XCTAssertNil(state.begin(now: 130, busy: true))
        XCTAssertEqual(state.begin(now: 131, busy: false), 1)
        state.observe(revision: 2, dirty: true, now: 135)
        state.finish(revision: 1, success: true, now: 140)
        XCTAssertEqual(state.due, 165)
        XCTAssertEqual(state.begin(now: 165, busy: false), 2)
        state.finish(revision: 2, success: false, now: 170)
        XCTAssertEqual(state.due, 200)
        XCTAssertEqual(state.begin(now: 200, busy: false), 2)
        state.finish(revision: 2, success: true, now: 201)
        XCTAssertNil(state.due)
        state.observe(revision: 3, dirty: false, now: 205)
        XCTAssertNil(state.begin(now: 300, busy: false))
        state.observe(revision: 3, dirty: true, now: 310)
        XCTAssertEqual(state.due, 340)
    }
}
