import XCTest
@testable import Strata

@MainActor
final class PSDImportTests: XCTestCase {
    func testNativeWorkspaceOpensPSDWithEditableHierarchy() async throws {
        let source = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("crates/strata-core/tests/fixtures/psd/group-mask.psd")
        let engine = StrataEngine(listener: EngineEvents { _ in })
        let controller = MainWindowController(engine: engine)
        defer { controller.canvas.shutdown() }
        let result: Result<ImageInfo, Error> = await withCheckedContinuation { continuation in
            controller.open(source, recordRecent: false) { continuation.resume(returning: $0) }
        }
        XCTAssertEqual(try result.get().bitDepth, 16)
        let state = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(state.layers.map(\.name), ["Base", "细节", "Exposure", "Invert", "Group"])
        let detail = try XCTUnwrap(state.layers.first { $0.name == "细节" })
        XCTAssertEqual(detail.parent, try XCTUnwrap(state.layers.last).id)
        XCTAssertNotNil(detail.mask)
        XCTAssertEqual(state.layers.first { $0.name == "Exposure" }?.exposure, 1.5)
        XCTAssertTrue(state.history.isEmpty)
        XCTAssertNil(engine.projectPath())
    }
}
