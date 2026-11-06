import AppKit
import XCTest
@testable import Strata

final class CropInputTests:XCTestCase{
    @MainActor
    private func configured()throws->(StrataEngine,CanvasCropInput){
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":16,"bit_depth":16,"background":[10001,30003,50001,32769]]));let input=CanvasCropInput(engine:engine);input.bind(try EditorSnapshot.decode(engine.snapshotJson()));return(engine,input)
    }
    @MainActor
    private func finished(_ input:CanvasCropInput)async throws{for _ in 0..<300{if !input.running{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("crop worker did not finish")}
    @MainActor
    func testDraggingRatioAndCancellationNeverEditSelectionPixelsOrHistoryUntilConfirm()async throws{
        let(engine,input)=try configured();_ = try engine.dispatch(command:"select.all",paramsJson:"{}");input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.configure(enabled:true,ratio:"4:3");let before=try engine.snapshotJson()
        input.down(NSPoint(x:2.25,y:3.75));input.up(NSPoint(x:13.1,y:14.2));XCTAssertEqual(input.draft,CGRect(x:2,y:3,width:12,height:9));XCTAssertNotNil(input.path());XCTAssertEqual(try engine.snapshotJson(),before)
        input.cancel();XCTAssertNil(input.draft);XCTAssertFalse(input.confirm());XCTAssertEqual(try engine.snapshotJson(),before)
        input.down(NSPoint(x:2.25,y:3.75));input.up(NSPoint(x:13.1,y:14.2));XCTAssertTrue(input.confirm());try await finished(input);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.x,2);XCTAssertEqual(after.frame?.y,3);XCTAssertEqual(after.frame?.width,12);XCTAssertEqual(after.frame?.height,9);XCTAssertEqual(after.history.count,2);XCTAssertEqual(after.history.last?.command,"image.crop");XCTAssertNil(input.draft);XCTAssertTrue(input.error.isEmpty,input.error)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).frame?.width,16)
    }
    @MainActor
    func testNewDocumentAndCancelBeforeQueuedApplyCannotRetargetDraft()async throws{
        let(engine,input)=try configured();input.configure(enabled:true,ratio:"free");input.down(NSPoint(x:2,y:3));input.up(NSPoint(x:12,y:13));XCTAssertTrue(input.confirm());input.cancel();try await finished(input);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
        input.down(NSPoint(x:1,y:1));input.up(NSPoint(x:12,y:12));_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":4,"bit_depth":16,"discard_changes":true]));input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertNil(input.draft);XCTAssertFalse(input.confirm());XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
        // Switching tools cancels only this owned crop session.
        input.down(NSPoint(x:0,y:0));input.up(NSPoint(x:3,y:3));input.configure(enabled:false,ratio:"free");XCTAssertFalse(input.hasDraft);XCTAssertFalse(input.confirm());input.down(NSPoint(x:0,y:0));XCTAssertFalse(input.hasDraft)
    }
}
