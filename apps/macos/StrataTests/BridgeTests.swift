import AppKit
import QuartzCore
import XCTest
@testable import Strata

@MainActor
final class BridgeTests: XCTestCase {
    func testCoreVersion() {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        XCTAssertEqual(engine.coreVersion(), "0.0.1")
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
