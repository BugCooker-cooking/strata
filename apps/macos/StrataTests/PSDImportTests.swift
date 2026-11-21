import AppKit
import ImageIO
import XCTest
@testable import Strata

@MainActor
final class PSDImportTests: XCTestCase {
    func testExportMenuWritesReadableCopyAndKeepsDocumentEditable() async throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        let controller = MainWindowController(engine: engine)
        defer { controller.canvas.shutdown() }
        let item = NSMenuItem(title: "", action: #selector(MainWindowController.exportLayeredPSD), keyEquivalent: "")
        XCTAssertFalse(controller.validateMenuItem(item))
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON([
            "width": 5, "height": 4, "bit_depth": 8, "background": [30, 120, 90, 128]
        ]))
        _ = try engine.dispatch(command: "layer.create", paramsJson: commandJSON(["name": "Editable"]))
        controller.refresh()
        XCTAssertTrue(controller.validateMenuItem(item))
        let before = try EditorSnapshot.decode(engine.snapshotJson())
        let output = FileManager.default.temporaryDirectory.appendingPathComponent("strata-\(UUID()).psd")
        defer { try? FileManager.default.removeItem(at: output) }
        let completed = await withCheckedContinuation { continuation in
            controller.writeLayeredPSD(to: output) { continuation.resume(returning: $0) }
        }
        XCTAssertTrue(completed)
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(output as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        XCTAssertEqual(image.width, 5)
        XCTAssertEqual(image.height, 4)
        let after = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(after.revision, before.revision)
        XCTAssertEqual(after.historyCursor, before.historyCursor)
        XCTAssertEqual(after.dirty, before.dirty)
        let imported = StrataEngine(listener: EngineEvents { _ in })
        _ = try imported.openImage(path: output.path)
        XCTAssertEqual(try EditorSnapshot.decode(imported.snapshotJson()).layers.map(\.name), ["Background", "Editable"])
    }

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
