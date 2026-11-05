import Foundation
import XCTest
@testable import Strata

final class TonalPreviewTests: XCTestCase {
    @MainActor
    private func configured() throws -> (StrataEngine,TonalPreviewModel) {
        let engine=StrataEngine(listener:EngineEvents{_ in})
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"background":[20000,20001,20002,65535]]))
        _ = try engine.dispatch(command:"layer.add_tonal_adjustment",paramsJson:commandJSON(["name":"tone","adjustment":["type":"brightness_contrast"]]))
        let preview=TonalPreviewModel(engine:engine);preview.bind(try EditorSnapshot.decode(engine.snapshotJson()))
        return (engine,preview)
    }
    private func parameters(_ value:Double) throws -> String {try commandJSON(["type":"brightness_contrast","brightness":value])}
    @MainActor
    private func idle(_ preview:TonalPreviewModel) async throws {
        for _ in 0..<300 {try await Task.sleep(for:.milliseconds(10));if !preview.busy && !preview.applying{return}}
        XCTFail("Tonal preview worker did not become idle")
    }
    @MainActor
    func testRapidPreviewEditsKeepDocumentHistoryAndApplyOnlyTheLatestSettings() async throws {
        let (engine,preview)=try configured();let before=try engine.snapshotJson()
        var applyChanges:[Bool]=[];preview.onApplying={applyChanges.append($0)}
        preview.setEnabled(true,settings:try parameters(1))
        for value in 2...20 {preview.update(try parameters(Double(value)))}
        try await idle(preview)
        XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertTrue(preview.error.isEmpty);XCTAssertTrue(applyChanges.isEmpty)
        preview.apply(try parameters(25));try await idle(preview)
        let state=try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(state.history.count,2);XCTAssertEqual(state.layers.last?.kind?.adjustment?["brightness"].number,25)
        XCTAssertEqual(applyChanges,[true,false]);XCTAssertFalse(preview.enabled);XCTAssertTrue(preview.error.isEmpty)
    }
    @MainActor
    func testCancelBeforeDebounceAndContextChangeCannotCommitOrRetarget() async throws {
        let (engine,preview)=try configured();let before=try engine.snapshotJson()
        preview.setEnabled(true,settings:try parameters(10));preview.cancel();try await idle(preview)
        XCTAssertEqual(try engine.snapshotJson(),before)
        preview.setEnabled(true,settings:try parameters(15))
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":4,"bit_depth":16,"discard_changes":true]))
        let next=try EditorSnapshot.decode(engine.snapshotJson());preview.bind(next);try await idle(preview)
        XCTAssertFalse(preview.enabled);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).documentId,next.documentId)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
    }
    @MainActor
    func testApplyBeforeFirstPreviewAndValidationErrorKeepOneHistoryBoundary() async throws {
        let (engine,preview)=try configured()
        preview.setEnabled(true,settings:try parameters(10));preview.apply(try parameters(30));try await idle(preview)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,2)
        preview.bind(try EditorSnapshot.decode(engine.snapshotJson()))
        preview.setEnabled(true,settings:try parameters(500));try await idle(preview)
        XCTAssertFalse(preview.error.isEmpty);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,2)
        preview.cancel();try await idle(preview)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).layers.last?.kind?.adjustment?["brightness"].number,30)
    }
    @MainActor
    func testQuickOffOnSequenceRestartsPreviewWithoutCommit() async throws {
        let (engine,preview)=try configured();let before=try engine.snapshotJson()
        preview.setEnabled(true,settings:try parameters(5));try await idle(preview)
        preview.cancel();preview.setEnabled(true,settings:try parameters(15));try await idle(preview)
        XCTAssertTrue(preview.enabled);XCTAssertTrue(preview.error.isEmpty)
        XCTAssertEqual(try engine.snapshotJson(),before)
        preview.apply(try parameters(25));try await idle(preview)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).layers.last?.kind?.adjustment?["brightness"].number,25)
    }
}
