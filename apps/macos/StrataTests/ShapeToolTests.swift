import AppKit
import XCTest
@testable import Strata

@MainActor
final class ShapeToolTests: XCTestCase {
    func testCanvasDragCreatesOneShapeAndInspectorReeditsItAfterReload() async throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 80, "height": 64, "bit_depth": 16]))
        let controller = MainWindowController(engine: engine)
        let window = try XCTUnwrap(controller.window)
        defer { controller.canvas.shutdown(); window.orderOut(nil) }
        controller.showWindow(nil); window.contentView?.layoutSubtreeIfNeeded()
        controller.refresh(); controller.canvas.fit()
        controller.workspace.selectTool(.shape)
        let input = controller.workspace.shapeInput
        input.tool = .rounded; input.radius = 3
        let canvas = controller.canvas
        let zero = try XCTUnwrap(canvas.documentPoint(.zero))
        let unit = try XCTUnwrap(canvas.documentPoint(CGPoint(x: 1,y: 1)))
        func send(_ type: NSEvent.EventType, _ x: CGFloat, _ y: CGFloat, _ modifiers: NSEvent.ModifierFlags = []) throws {
            let p = CGPoint(x: (x-zero.x)/(unit.x-zero.x), y: (y-zero.y)/(unit.y-zero.y))
            let event = try XCTUnwrap(NSEvent.mouseEvent(with: type, location: canvas.convert(p, to: nil),
                modifierFlags: modifiers, timestamp: 0, windowNumber: window.windowNumber, context: nil,
                eventNumber: 1, clickCount: 1, pressure: 1))
            switch type {
            case .leftMouseDown: canvas.mouseDown(with: event)
            case .leftMouseDragged: canvas.mouseDragged(with: event)
            default: canvas.mouseUp(with: event)
            }
        }
        func settle() async throws {
            for _ in 0..<250 where controller.busy { try await Task.sleep(for: .milliseconds(10)) }
            XCTAssertFalse(controller.busy); controller.refresh()
        }
        let history = try XCTUnwrap(controller.state?.historyCursor)
        try send(.leftMouseDown, 10, 8); try send(.leftMouseDragged, 40, 28, [.shift])
        XCTAssertNotNil(input.preview)
        XCTAssertEqual(controller.state?.historyCursor, history)
        try send(.leftMouseUp, 40, 28, [.shift]); try await settle()
        XCTAssertEqual(controller.state?.historyCursor, history+1)
        XCTAssertEqual(input.end.x-input.start.x, 30, accuracy: 0.001)
        XCTAssertEqual(input.end.y-input.start.y, 30, accuracy: 0.001)
        let target = try XCTUnwrap(input.target)
        input.radius = 5; input.apply(create: false); try await settle()
        let path = try XCTUnwrap(controller.state?.paths?.last)
        XCTAssertEqual(path.shape?.geometry.primitive.radius, 5)
        XCTAssertEqual(path.anchors.count, 8)
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("shape-\(UUID()).strata")
        defer { try? FileManager.default.removeItem(at: file) }
        let token = try engine.grantWrite(path: file.path, overwrite: false)
        _ = try engine.dispatch(command: "file.save_as", paramsJson: commandJSON(["resource":token]))
        let read = try engine.grantRead(path: file.path)
        _ = try engine.dispatch(command: "file.open", paramsJson: commandJSON(["resource":read]))
        controller.refresh()
        XCTAssertEqual(input.target, target); XCTAssertEqual(input.tool, .rounded); XCTAssertEqual(input.radius, 5)
        input.tool = .line
        input.down(CGPoint(x: 1, y: 1)); input.move(CGPoint(x: 20,y: 3), constrain: true)
        XCTAssertEqual(input.end.y, 1, accuracy: 0.00001)
        input.cancel()
        XCTAssertNil(input.preview)
        XCTAssertEqual(controller.state?.layers.count, 2)
    }
}
