import AppKit
import QuartzCore
import UniformTypeIdentifiers
import XCTest
@testable import Strata

@MainActor
final class BridgeTests: XCTestCase {
    func testProjectTypeKeepsSingleExtensionInSavePanel() throws {
        let type = try XCTUnwrap(UTType(filenameExtension: "strata"))
        XCTAssertEqual(type.identifier, "dev.strata-editor.project")
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "example.strata"
        panel.allowedContentTypes = [type]
        XCTAssertEqual(panel.nameFieldStringValue, "example.strata")
    }

    func testCoreVersion() {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        XCTAssertEqual(engine.coreVersion(), "0.0.1")
    }

    func testCommandHistoryProjectRoundtripAndExport() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let engine = StrataEngine(listener: EngineEvents { _ in })
        func invoke(_ id: String, _ params: [String: Any]) throws {
            _ = try engine.dispatch(command: id, paramsJson: commandJSON(params))
        }
        try invoke("file.new", ["width": 16, "height": 16, "bit_depth": 16])
        try invoke("layer.create", ["name": "颜色", "fill": [32768, 32769, 50001, 65535]])
        try invoke("layer.set_opacity", ["layer": ["name": "颜色"], "opacity": 0.5])
        try invoke("edit.undo", [:])
        let before = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(before.layers.count, 2)
        XCTAssertEqual(before.layers.last?.opacity, 1)
        let project = directory.appendingPathComponent("roundtrip.strata")
        let target = try engine.grantWrite(path: project.path, overwrite: false)
        try invoke("file.save_as", ["resource": target])
        XCTAssertFalse(try EditorSnapshot.decode(engine.snapshotJson()).dirty)
        try invoke("layer.rename", ["layer": ["name": "颜色"], "name": "修改"])
        let input = try engine.grantRead(path: project.path)
        try invoke("file.open", ["resource": input, "discard_changes": true])
        let reopened = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(reopened.layers.map(\.id), before.layers.map(\.id))
        XCTAssertEqual(reopened.layers.last?.name, "颜色")
        XCTAssertEqual(reopened.bitDepth, 16)
        for format in ["png", "tiff"] {
            let output = directory.appendingPathComponent("export.\(format)")
            let resource = try engine.grantWrite(path: output.path, overwrite: false)
            try invoke("file.export", ["resource": resource, "format": format])
            let imported = StrataEngine(listener: EngineEvents { _ in })
            XCTAssertEqual(try imported.openImage(path: output.path).bitDepth, 16)
        }
    }

    func testOpenImageDeliversMainThreadEvent() async throws {
        let opened = expectation(description: "document opened")
        let events = EngineEvents { event in
            XCTAssertTrue(Thread.isMainThread)
            if case let .documentOpened(info, generation) = event {
                XCTAssertEqual(info.width, 1024)
                XCTAssertEqual(info.height, 1536)
                XCTAssertEqual(info.bitDepth, 8)
                XCTAssertGreaterThan(generation, 0)
                opened.fulfill()
            }
        }
        let engine = StrataEngine(listener: events)
        let url = try XCTUnwrap(Bundle.main.url(forResource: "sample-photo", withExtension: "jpg"))
        _ = try await Task.detached { try engine.openImage(path: url.path) }.value
        await fulfillment(of: [opened], timeout: 5)
    }

    func testMissingFileDoesNotReplaceDocument() throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        let url = try XCTUnwrap(Bundle.main.url(forResource: "sample-photo", withExtension: "jpg"))
        let original = try engine.openImage(path: url.path)
        XCTAssertThrowsError(try engine.openImage(path: "/nonexistent/strata-test.png")) { error in
            guard case StrataError.NotFound = error else { return XCTFail("\(error)") }
        }
        XCTAssertEqual(engine.documentInfo(), original)
    }

    func testMetalLayerAttachResizeDetachPreservesDocument() throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        let url = try XCTUnwrap(Bundle.main.url(forResource: "sample-photo", withExtension: "jpg"))
        let original = try engine.openImage(path: url.path)
        let layer = CAMetalLayer()
        layer.colorspace = CGColorSpace(name: CGColorSpace.displayP3)
        let pointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(layer).toOpaque()))
        defer { try? engine.detachCanvas() }
        do {
            try engine.attachCanvas(metalLayerPtr: pointer, viewport: CanvasViewport(width: 64, height: 64, scale: 1))
        } catch StrataError.Render(let message) where message.contains("no GPU adapter") {
            if ProcessInfo.processInfo.environment["STRATA_REQUIRE_GPU"] != nil { throw BenchmarkFailure(message) }
            throw XCTSkip("No GPU adapter on this test host; local Metal validation is still required")
        }
        try engine.resizeCanvas(viewport: CanvasViewport(width: 0, height: 0, scale: 1))
        XCTAssertTrue(try engine.renderFrame().skipped)
        try engine.resizeCanvas(viewport: CanvasViewport(width: 128, height: 128, scale: 2))
        try engine.detachCanvas()
        try engine.detachCanvas()
        XCTAssertEqual(engine.documentInfo(), original)
        XCTAssertThrowsError(try engine.renderFrame())
        // Explicitly keep the layer alive through synchronous detach.
        withExtendedLifetime(layer) {}
    }
}
