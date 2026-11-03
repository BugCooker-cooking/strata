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
}
