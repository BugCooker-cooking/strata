import AppKit
import XCTest
@testable import Strata

@MainActor
final class NewDocumentOptionsTests: XCTestCase {
    func testPresetsDepthSpaceAndBackgroundProduceTheCoreContract() throws {
        let alert = NSAlert(); alert.addButton(withTitle: "创建")
        let options = NewDocumentOptionsController(alert: alert)
        options.preset.selectItem(at: 2); options.changePreset()
        XCTAssertEqual(try options.parameters()["width"] as? UInt32, 2480)
        XCTAssertEqual(try options.parameters()["height"] as? UInt32, 3508)
        XCTAssertEqual(try options.parameters()["resolution_dpi"] as? UInt32, 300)
        XCTAssertEqual(try options.parameters()["preset"] as? String, "a4_300dpi")
        options.depth.selectItem(at: 1); options.changeDepth(); options.space.selectItem(at: 1)
        options.background.selectItem(at: 3); options.changeBackground()
        XCTAssertEqual(try options.parameters()["background"] as? [Int], [65535,65535,65535,65535])
        XCTAssertEqual(try options.parameters()["working_space"] as? String, "display_p3")
        options.channels[0].stringValue = "32768"; options.channels[1].stringValue = "32769"
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(options.parameters()))
        let snapshot = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(snapshot.resolutionDpi, 300); XCTAssertEqual(snapshot.bitDepth, 16)
    }
    func testInvalidInputDisablesCreateAndCustomEditsClearPreset() throws {
        let alert = NSAlert(); let create = alert.addButton(withTitle: "创建")
        let options = NewDocumentOptionsController(alert: alert)
        options.preset.selectItem(at: 1); options.changePreset()
        options.width.stringValue = "512"; options.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: options.width))
        XCTAssertEqual(options.preset.indexOfSelectedItem, 0)
        XCTAssertNil(try options.parameters()["preset"])
        for invalid in ["", "0", "-1", "0.5", "4294967296"] {
            options.width.stringValue = invalid; options.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: options.width))
            XCTAssertFalse(create.isEnabled); XCTAssertThrowsError(try options.parameters())
        }
        options.width.stringValue = "512"; options.resolution.stringValue = "2401"
        XCTAssertThrowsError(try options.parameters())
        options.resolution.stringValue = "300"; options.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: options.resolution))
        XCTAssertTrue(create.isEnabled)
    }
}
