import AppKit
import XCTest
@testable import Strata

@MainActor
final class WorkPathTests: XCTestCase {
    func testCanvasPenEventsCommitOnePathAndFillViaWorkspace() async throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 80, "height": 64, "bit_depth": 16]))
        let controller = MainWindowController(engine: engine)
        let window = try XCTUnwrap(controller.window)
        defer { controller.canvas.shutdown(); window.orderOut(nil) }
        controller.showWindow(nil)
        window.contentView?.layoutSubtreeIfNeeded()
        controller.refresh()
        controller.canvas.fit()
        controller.workspace.selectTool(.pen)
        let canvas = controller.canvas
        let zero = try XCTUnwrap(canvas.documentPoint(.zero))
        let unit = try XCTUnwrap(canvas.documentPoint(CGPoint(x: 1, y: 1)))
        func send(_ type: NSEvent.EventType, _ x: CGFloat, _ y: CGFloat) throws {
            let point = CGPoint(x: (x-zero.x)/(unit.x-zero.x), y: (y-zero.y)/(unit.y-zero.y))
            let event = try XCTUnwrap(NSEvent.mouseEvent(with: type, location: canvas.convert(point, to: nil),
                modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil,
                eventNumber: 1, clickCount: 1, pressure: 1))
            switch type {
            case .leftMouseDown: canvas.mouseDown(with: event)
            case .leftMouseDragged: canvas.mouseDragged(with: event)
            default: canvas.mouseUp(with: event)
            }
        }
        func settle() async throws {
            for _ in 0..<200 where controller.busy { try await Task.sleep(for: .milliseconds(10)) }
            XCTAssertFalse(controller.busy)
        }
        try send(.leftMouseDown, 10, 10); try send(.leftMouseUp, 10, 10)
        try send(.leftMouseDown, 64, 12); try send(.leftMouseDragged, 70, 18); try send(.leftMouseUp, 70, 18)
        try send(.leftMouseDown, 36, 52); try send(.leftMouseUp, 36, 52)
        try send(.leftMouseDown, 10, 10); try send(.leftMouseUp, 10, 10)
        try await settle()
        let saved = try XCTUnwrap(controller.state?.paths?.first)
        XCTAssertEqual(saved.anchors.count, 3)
        XCTAssertEqual(saved.anchors[1].outgoing.x, 6, accuracy: 0.0001)
        XCTAssertEqual(controller.workspace.pathInput.selectedID, saved.id)
        let history = try XCTUnwrap(controller.state?.historyCursor)
        controller.workspace.send(.command("layer.fill.path", ["path": ["id": saved.id],
            "name": "Filled", "color": [1200, 26000, 55000, 65535]]))
        try await settle()
        XCTAssertNotNil(controller.state?.layers.last?.mask)
        XCTAssertEqual(controller.state?.historyCursor, history+1)
    }

    func testTracePanelEditsHoleIndependentlyAndTranslatesAllContours() async throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        func command(_ id: String, _ params: [String: Any]) throws {
            _ = try engine.dispatch(command: id, paramsJson: commandJSON(params))
        }
        let controller = MainWindowController(engine: engine)
        defer { controller.canvas.shutdown() }
        let input = controller.workspace.pathInput
        func settle() async throws {
            for _ in 0..<200 where controller.busy { try await Task.sleep(for: .milliseconds(10)) }
            XCTAssertFalse(controller.busy)
            controller.refresh()
        }
        try command("file.new", ["width": 50, "height": 40, "bit_depth": 8])
        try command("select.marquee", ["x": 5, "y": 4, "width": 30, "height": 26])
        try command("select.marquee", ["x": 12, "y": 10, "width": 10, "height": 8, "mode": "subtract"])
        controller.refresh()
        controller.workspace.selectTool(.pen)
        input.trace(threshold: 32768, tolerance: 0)
        try await settle()
        XCTAssertNotNil(input.selectedID)
        XCTAssertEqual(input.contours.map(\.count), [4,4])
        let outer = input.contours[0]
        input.chooseContour(1)
        input.down(input.anchors[0].position, tolerance: 1, modifiers: [], clicks: 1)
        input.endDrag()
        input.splitNext()
        try await settle()
        XCTAssertEqual(input.contour, 1)
        XCTAssertEqual(input.contours[0], outer)
        XCTAssertEqual(input.anchors.count, 5)
        input.down(.zero, tolerance: 1, modifiers: [.command], clicks: 1)
        input.move(CGPoint(x: 2, y: 3))
        input.endDrag()
        input.apply()
        try await settle()
        XCTAssertEqual(input.contours[0][0].position.x, outer[0].position.x + 2)
        input.removeContour()
        input.apply()
        try await settle()
        XCTAssertEqual(input.contours.count, 1)
        try command("edit.undo", [:])
        try await settle()
        XCTAssertEqual(input.contours.map(\.count), [4,5])
    }

    func testOpenContourAndLinkedTangentAreEditableAfterPSDInterchange() throws {
        let source = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("crates/strata-core/tests/fixtures/psd/saved-path.psd")
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.openImage(path: source.path)
        let input = CanvasPathInput()
        input.onCommand = { id, params in
            do { _ = try engine.dispatch(command: id, paramsJson: commandJSON(params)) }
            catch { XCTFail("Path edit failed: \(error)") }
        }
        func refresh() throws { input.bind(try EditorSnapshot.decode(engine.snapshotJson()), enabled: true) }
        try refresh()
        let path = try XCTUnwrap(input.paths.first)
        XCTAssertEqual(path.name, "輪廓 Café")
        input.select(path.id)
        input.chooseContour(1)
        XCTAssertTrue(input.openContours.contains(1))
        XCTAssertEqual(input.anchors.count, 2)
        let at = input.anchors[0].position
        input.down(at, tolerance: 0.01, modifiers: [], clicks: 1)
        input.endDrag()
        input.down(CGPoint(x: at.x+2, y: at.y+1), tolerance: 0.01, modifiers: [], clicks: 1)
        input.move(CGPoint(x: at.x, y: at.y+2))
        input.endDrag()
        XCTAssertEqual(input.anchors[0].incoming.x, 0, accuracy: 0.0001)
        XCTAssertEqual(input.anchors[0].incoming.y, -sqrt(5), accuracy: 0.0001)
        input.apply()
        try refresh()
        let output = FileManager.default.temporaryDirectory.appendingPathComponent("strata-open-\(UUID()).psd")
        defer { try? FileManager.default.removeItem(at: output) }
        let grant = try engine.grantWrite(path: output.path, overwrite: false)
        _ = try engine.dispatch(command: "file.export_psd", paramsJson: commandJSON(["resource": grant]))
        let copy = StrataEngine(listener: EngineEvents { _ in })
        _ = try copy.openImage(path: output.path)
        let saved = try XCTUnwrap(try EditorSnapshot.decode(copy.snapshotJson()).paths?.first)
        XCTAssertEqual(saved.openContours, [1])
        XCTAssertEqual(saved.subpaths?[0][0].linked, true)
        XCTAssertEqual(try XCTUnwrap(saved.subpaths?[0][0].outgoing.y), 2, accuracy: 0.00001)
    }

    func testPenDraftEditsThroughTheEngineAndCancelsOnRevisionChange() throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        func command(_ id: String, _ params: [String: Any]) throws {
            _ = try engine.dispatch(command: id, paramsJson: commandJSON(params))
        }
        func snapshot() throws -> EditorSnapshot { try EditorSnapshot.decode(engine.snapshotJson()) }
        try command("file.new", ["width": 64, "height": 64, "bit_depth": 16])
        let input = CanvasPathInput()
        input.bind(try snapshot(), enabled: true)
        input.onCommand = { id, parameters in
            do { try command(id, parameters) }
            catch { XCTFail("Path command failed: \(error)") }
        }
        func add(_ x: CGFloat, _ y: CGFloat) {
            input.down(CGPoint(x: x, y: y), tolerance: 1, modifiers: [], clicks: 1)
            input.endDrag()
        }
        add(3, 3)
        input.down(CGPoint(x: 40, y: 4), tolerance: 1, modifiers: [], clicks: 1)
        input.move(CGPoint(x: 45, y: 9))
        input.endDrag()
        add(6, 40)
        XCTAssertTrue(input.canApply)
        input.apply()
        input.bind(try snapshot(), enabled: true)
        let id = try XCTUnwrap(input.selectedID)
        XCTAssertEqual(input.paths.count, 1)
        XCTAssertEqual(input.anchors[1].incoming, CGPoint(x: -5, y: -5))
        let base = try snapshot().historyCursor
        input.down(CGPoint(x: 40, y: 4), tolerance: 1, modifiers: [], clicks: 1)
        input.move(CGPoint(x: 38, y: 7))
        input.endDrag()
        input.apply()
        input.bind(try snapshot(), enabled: true)
        XCTAssertEqual(input.anchors[1].position, CGPoint(x: 38, y: 7))
        XCTAssertEqual(input.anchors[1].outgoing, CGPoint(x: 5, y: 5))
        XCTAssertEqual(try snapshot().historyCursor, base + 1)
        input.down(CGPoint(x: 38, y: 7), tolerance: 1, modifiers: [], clicks: 1)
        input.endDrag()
        input.splitNext()
        input.bind(try snapshot(), enabled: true)
        XCTAssertEqual(input.anchors.count, 4)
        input.down(CGPoint(x: 3, y: 3), tolerance: 1, modifiers: [], clicks: 1)
        input.endDrag()
        input.down(CGPoint(x: 38, y: 7), tolerance: 1, modifiers: [.shift], clicks: 1)
        XCTAssertEqual(input.selectedAnchors, [0, 1])
        input.down(CGPoint(x: 38, y: 7), tolerance: 1, modifiers: [], clicks: 1)
        input.move(CGPoint(x: 40, y: 8))
        XCTAssertEqual(input.anchors[0].position, CGPoint(x: 5, y: 4))
        input.endDrag()
        input.select(id)
        input.down(.zero, tolerance: 1, modifiers: [.command], clicks: 1)
        input.move(CGPoint(x: 5, y: 5))
        XCTAssertEqual(input.anchors.last?.position, CGPoint(x: 11, y: 45))
        input.endDrag()
        input.select(id)
        input.loadSelection()
        XCTAssertNotNil(try snapshot().selection)
        input.bind(try snapshot(), enabled: true)
        input.down(CGPoint(x: 38, y: 7), tolerance: 1, modifiers: [], clicks: 1)
        input.move(CGPoint(x: 50, y: 11))
        try command("layer.create", ["name": "External change"])
        input.bind(try snapshot(), enabled: true)
        XCTAssertFalse(input.canApply)
        XCTAssertEqual(input.selectedID, id)
        XCTAssertEqual(input.anchors[1].position, CGPoint(x: 38, y: 7))
    }
}
