import AppKit
import XCTest
@testable import Strata

final class SelectionInputTests: XCTestCase {
    @MainActor
    private func engine() throws -> StrataEngine {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 16, "height": 16, "bit_depth": 16, "background": [65535,0,0,65535]]))
        return engine
    }
    @MainActor
    private func finish(_ input: CanvasSelectionInput) async throws {
        for _ in 0..<200 {
            if !input.running { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTFail("Selection worker did not finish in two seconds")
    }
    @MainActor
    func testPolygonStagesUntilReturnAndEscapeCancelsWithoutHistory() async throws {
        let engine = try engine(), input = CanvasSelectionInput(engine: engine)
        input.configure(CanvasSelectionConfiguration(command: "select.polygon"))
        for point in [NSPoint(x:2,y:2), NSPoint(x:12,y:2), NSPoint(x:8,y:12)] {
            input.down(point, modifiers: [], clicks: 1); input.up(point)
        }
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 0)
        XCTAssertTrue(input.confirm()); try await finish(input)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 1)
        input.down(NSPoint(x:1,y:1), modifiers: [], clicks: 1); input.cancel()
        XCTAssertFalse(input.confirm())
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 1)
    }
    @MainActor
    func testEllipseAndWandCommitOnceAndQuickWaitsForBothBrushClasses() async throws {
        let engine = try engine(), input = CanvasSelectionInput(engine: engine)
        var failures: [String] = []
        input.onError = { failures.append($0.localizedDescription) }
        input.configure(CanvasSelectionConfiguration(command: "select.ellipse"))
        input.down(NSPoint(x:2,y:2), modifiers: [], clicks: 1)
        input.up(NSPoint(x:12,y:12)); try await finish(input)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 1)
        input.configure(CanvasSelectionConfiguration(command: "select.wand", tolerance: 0))
        input.down(NSPoint(x:4,y:4), modifiers: [], clicks: 1); try await finish(input)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 2)
        input.configure(CanvasSelectionConfiguration(command: "select.quick", radius: 0.5))
        input.down(NSPoint(x:8.5,y:8.5), modifiers: [], clicks: 1); input.up(NSPoint(x:8.5,y:8.5))
        input.paintBackground = true
        input.down(NSPoint(x:1.5,y:1.5), modifiers: [], clicks: 1); input.up(NSPoint(x:1.5,y:1.5))
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 2)
        XCTAssertTrue(input.confirm()); try await finish(input)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count, 3)
        XCTAssertTrue(failures.isEmpty, failures.joined(separator: "; "))
    }
}
