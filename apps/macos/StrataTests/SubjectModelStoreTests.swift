import Foundation
import XCTest
@testable import Strata

final class SubjectModelStoreTests: XCTestCase {
    func testInvalidOversizedAndSymlinkImportsPreserveInstalledFileAndCleanStaging() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Strata-model-import-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source.onnx"), target = root.appendingPathComponent("cache/model.onnx")
        try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        let original = Data("existing cached model sentinel".utf8)
        try original.write(to: target); try Data("abc".utf8).write(to: source)
        XCTAssertThrowsError(try SubjectModelInstallation.stage(source: source, destination: target, maximumBytes: 2))
        XCTAssertThrowsError(try SubjectModelInstallation.stage(source: source, destination: target, maximumBytes: 8*1024*1024))
        let link = root.appendingPathComponent("symlink.onnx")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: source)
        XCTAssertThrowsError(try SubjectModelInstallation.stage(source: link, destination: target, maximumBytes: 8*1024*1024))
        XCTAssertEqual(try Data(contentsOf: target), original)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: target.deletingLastPathComponent().path), ["model.onnx"])
    }
    func testAssetMetadataContainsExactSupportedVersionAndLicense() throws {
        let asset = try SubjectModelAsset.current()
        XCTAssertEqual(asset.id, "u2netp"); XCTAssertEqual(asset.version, "rembg-v0.0.0")
        XCTAssertEqual(asset.sha256, "309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8")
        XCTAssertTrue(asset.license.contains("Apache-2.0")); XCTAssertTrue(asset.license.contains("MIT"))
    }
}
