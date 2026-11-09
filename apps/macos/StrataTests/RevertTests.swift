import AppKit
import XCTest
@testable import Strata

@MainActor
final class RevertTests:XCTestCase{
    private func engine()->StrataEngine{StrataEngine(listener:EngineEvents{_ in})}
    func testSourceBindingAndActualProjectRevertRestoreLayersAndClearHistoryWithoutWritingFile()throws{
        let e=engine(),dir=FileManager.default.temporaryDirectory.appendingPathComponent("strata-revert-\(UUID().uuidString)");try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true);defer{try? FileManager.default.removeItem(at:dir)}
        _ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":6,"bit_depth":16,"background":[10001,30003,50001,32769]]));XCTAssertNil(e.revertSourcePath())
        let path=dir.appendingPathComponent("saved.strata"),token=try e.grantWrite(path:path.path,overwrite:false);_ = try e.dispatch(command:"file.save_as",paramsJson:commandJSON(["resource":token]));let before=try EditorSnapshot.decode(e.snapshotJson()),disk=try Data(contentsOf:path)
        let source=try XCTUnwrap(e.revertSourcePath()),actual=try FileManager.default.attributesOfItem(atPath:source),expected=try FileManager.default.attributesOfItem(atPath:path.path)
        XCTAssertEqual(try XCTUnwrap(actual[.systemNumber] as? NSNumber),try XCTUnwrap(expected[.systemNumber] as? NSNumber));XCTAssertEqual(try XCTUnwrap(actual[.systemFileNumber] as? NSNumber),try XCTUnwrap(expected[.systemFileNumber] as? NSNumber))
        let info=try AIJSON.decode(e.dispatch(command:"file.revert_status",paramsJson:"{}"));XCTAssertEqual(info["name"].string,"saved.strata");XCTAssertEqual(info["source_kind"].string,"saved_project");XCTAssertNil(info["path"].string)
        _ = try e.dispatch(command:"layer.create",paramsJson:commandJSON(["name":"Discard unsaved"]));let changed=try EditorSnapshot.decode(e.snapshotJson()),read=try e.grantRead(path:path.path);_ = try e.dispatch(command:"file.revert",paramsJson:commandJSON(["resource":read,"discard_changes":true,"expected_revision":changed.revision]));let after=try EditorSnapshot.decode(e.snapshotJson())
        XCTAssertEqual(after.documentId,before.documentId);XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.layers.map(\.name),before.layers.map(\.name));XCTAssertEqual(after.layers.map(\.x),before.layers.map(\.x));XCTAssertEqual(after.layers.map(\.y),before.layers.map(\.y));XCTAssertTrue(after.history.isEmpty);XCTAssertFalse(after.dirty);XCTAssertEqual(try Data(contentsOf:path),disk)
    }
    func testMenuRequiresBoundSourceAndRevertRejectsUnapprovedDiscardAndDifferentFile()throws{
        let e=engine(),controller=MainWindowController(engine:e);let item=NSMenuItem(title:"恢复",action:#selector(MainWindowController.revertDocument),keyEquivalent:"");XCTAssertFalse(controller.validateMenuItem(item))
        _ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":6,"bit_depth":8]));controller.refresh();XCTAssertFalse(controller.validateMenuItem(item))
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent("strata-revert-menu-\(UUID().uuidString)");try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true);defer{try? FileManager.default.removeItem(at:dir);controller.canvas.shutdown()}
        let path=dir.appendingPathComponent("saved.strata"),other=dir.appendingPathComponent("other.strata");for url in [other,path]{let write=try e.grantWrite(path:url.path,overwrite:false);_ = try e.dispatch(command:"file.save_as",paramsJson:commandJSON(["resource":write]))};controller.refresh();XCTAssertTrue(controller.validateMenuItem(item))
        let read=try e.grantRead(path:path.path),wrong=try e.grantRead(path:other.path),truth=try e.snapshotJson();XCTAssertThrowsError(try e.dispatch(command:"file.revert",paramsJson:commandJSON(["resource":read])));XCTAssertThrowsError(try e.dispatch(command:"file.revert",paramsJson:commandJSON(["resource":wrong,"discard_changes":true])));XCTAssertEqual(try e.snapshotJson(),truth)
    }
}
