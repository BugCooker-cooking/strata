import AppKit
import XCTest
@testable import Strata

@MainActor
final class ExportOptionsTests: XCTestCase {
    func testSixteenBitQuantizationRequiresConfirmationAndFormatsKeepExtensionInSync() throws {
        let panel = NSSavePanel(); panel.nameFieldStringValue = "artwork.png"
        let options = ExportOptionsController(panel: panel, bitDepth: 16)
        XCTAssertEqual(try options.parameters()["format"] as? String, "png")
        XCTAssertFalse(options.quantization.isEnabled)
        options.format.selectItem(at: 2); options.changeFormat(options.format)
        XCTAssertEqual(panel.nameFieldStringValue, "artwork.jpg")
        XCTAssertThrowsError(try options.parameters())
        options.quantization.state = .on
        XCTAssertEqual(try options.parameters()["allow_quantization"] as? Bool, true)
        XCTAssertEqual(try options.parameters()["background"] as? [Int], [255,255,255])
        options.format.selectItem(at: 0); options.changeFormat(options.format)
        XCTAssertEqual(panel.nameFieldStringValue, "artwork.png")
        XCTAssertEqual(options.quantization.state, .off)
        XCTAssertNil(try options.parameters()["allow_quantization"])
    }

    func testQualityIsValidatedAndWebPIsExplicitlyLossless() throws {
        let options = ExportOptionsController(panel: NSSavePanel(), bitDepth: 8)
        options.format.selectItem(at: 2); options.changeFormat(options.format)
        for invalid in ["0", "101", "90.5", "bad"] {
            options.quality.stringValue = invalid; XCTAssertThrowsError(try options.parameters())
        }
        options.quality.stringValue = "85"; options.space.selectItem(at: 1)
        XCTAssertEqual(try options.parameters()["quality"] as? Int,85)
        XCTAssertEqual(try options.parameters()["target_space"] as? String,"srgb")
        options.format.selectItem(at: 3); options.changeFormat(options.format)
        XCTAssertFalse(options.quality.isEnabled)
        XCTAssertNil(try options.parameters()["quality"])
        XCTAssertNil(try options.parameters()["background"])
        XCTAssertNil(try options.parameters()["allow_quantization"])
    }
    func testResizeDimensionsAndAspectLockRejectInvalidOrUnboundedOutput() throws {
        let options = ExportOptionsController(panel: NSSavePanel(), bitDepth: 16, width: 4000, height: 3000)
        XCTAssertNil(try options.parameters()["width"])
        options.width.stringValue = "2000"
        options.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: options.width))
        XCTAssertEqual(options.height.stringValue, "1500")
        XCTAssertEqual(try options.parameters()["width"] as? UInt32, 2000)
        options.lockAspect.state = .off; options.height.stringValue = "500"
        XCTAssertEqual(try options.parameters()["height"] as? UInt32, 500)
        options.width.stringValue = "1"; options.lockAspect.state = .on; options.changeAspect()
        XCTAssertEqual(options.height.stringValue, "1")
        for invalid in ["0", "-1", "0.5", "4294967296", "bad"] {
            options.width.stringValue = invalid; XCTAssertThrowsError(try options.parameters())
        }
        options.width.stringValue = "100000"; options.height.stringValue = "100000"
        XCTAssertThrowsError(try options.parameters())
    }
    func testLossyWebPModeHasQualityAndKeepsJpegMatteOutOfItsPayload() throws {
        let options = ExportOptionsController(panel: NSSavePanel(), bitDepth: 16)
        options.format.selectItem(at: 3); options.changeFormat(options.format)
        options.webpMode.selectItem(at: 1); options.changeEncoding()
        XCTAssertTrue(options.quality.isEnabled); XCTAssertThrowsError(try options.parameters())
        options.quantization.state = .on; options.quality.stringValue = "83"
        XCTAssertEqual(try options.parameters()["webp_lossless"] as? Bool, false)
        XCTAssertEqual(try options.parameters()["quality"] as? Int, 83)
        XCTAssertNil(try options.parameters()["background"])
        options.webpMode.selectItem(at: 0); options.changeEncoding()
        XCTAssertFalse(options.quality.isEnabled); XCTAssertNil(try options.parameters()["quality"])
    }
}
