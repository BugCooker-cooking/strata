import AppKit
import XCTest
@testable import Strata

final class StraightenInputTests:XCTestCase{
    @MainActor
    private func configured()throws->(StrataEngine,CanvasStraightenInput){
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":40,"height":26,"bit_depth":16,"background":[10001,30003,50001,65535]]));let input=CanvasStraightenInput(engine:engine);input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.configure(enabled:true);return(engine,input)
    }
    @MainActor
    private func finished(_ input:CanvasStraightenInput)async throws{for _ in 0..<400{if !input.running{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("straighten worker did not finish")}
    @MainActor
    func testDraggingOnlyDrawsGuideAndOneConfirmationCommitsRotationAndCrop()async throws{
        let(engine,input)=try configured();_ = try engine.dispatch(command:"select.all",paramsJson:"{}");input.bind(try EditorSnapshot.decode(engine.snapshotJson()));let before=try engine.snapshotJson()
        input.down(NSPoint(x:2.25,y:4.75));input.drag(NSPoint(x:20.5,y:6));input.up(NSPoint(x:32.25,y:7.75));XCTAssertEqual(input.draft,StraightenLine(start:NSPoint(x:2.25,y:4.75),end:NSPoint(x:32.25,y:7.75)));XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertNotNil(input.path());XCTAssertTrue(input.canConfirm);XCTAssertEqual(try XCTUnwrap(input.correction),-atan(0.1)*180/Double.pi,accuracy:1e-10)
        XCTAssertTrue(input.confirm());try await finished(input);let after=try EditorSnapshot.decode(engine.snapshotJson());let original=try EditorSnapshot.decode(before);XCTAssertEqual(after.historyCursor,original.historyCursor+1);XCTAssertEqual(after.history.last?.command,"image.straighten");XCTAssertLessThan(after.frame?.height ?? 26,26);XCTAssertEqual(after.layers.map(\.id),original.layers.map(\.id));XCTAssertNil(input.draft);XCTAssertFalse(input.hasDraft);XCTAssertTrue(input.error.isEmpty,input.error)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.frame?.width,40);XCTAssertEqual(undone.frame?.height,26);XCTAssertEqual(undone.historyCursor,original.historyCursor)
    }
    @MainActor
    func testSteepLineCanChangeAxisAndSamplingWithoutRetargetingTheDraft()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson();input.down(NSPoint(x:4,y:2));input.up(NSPoint(x:7,y:24));XCTAssertFalse(input.canConfirm);XCTAssertNil(input.correction);XCTAssertFalse(input.error.isEmpty);XCTAssertTrue(input.confirm());XCTAssertEqual(try engine.snapshotJson(),before)
        let line=input.draft;input.configure(enabled:true,align:"vertical",autoCrop:false,interpolation:"nearest");XCTAssertEqual(input.draft,line);XCTAssertTrue(input.canConfirm);XCTAssertGreaterThan(try XCTUnwrap(input.correction),0);XCTAssertTrue(input.error.isEmpty)
        XCTAssertTrue(input.confirm());try await finished(input);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertGreaterThan(after.frame?.width ?? 0,40);XCTAssertGreaterThan(after.frame?.height ?? 0,26);XCTAssertEqual(after.history.count,1)
    }
    @MainActor
    func testCancelBeforeQueuedApplyToolSwitchAndNewDocumentKeepTruth()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson();input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:32,y:7));XCTAssertTrue(input.confirm());input.cancel();try await finished(input);XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertFalse(input.hasDraft)
        input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:32,y:7));input.configure(enabled:false);XCTAssertFalse(input.hasDraft);XCTAssertFalse(input.confirm());input.down(NSPoint(x:2,y:4));XCTAssertFalse(input.hasDraft)
        input.configure(enabled:true);input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:32,y:7));_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"discard_changes":true]));input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertNil(input.path());XCTAssertFalse(input.confirm());XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
    }
    @MainActor
    func testLateDraftAfterRevisionChangeCannotCommitAndPaletteChangesKeepTheOriginalGuide()async throws{
        let(engine,input)=try configured();input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:32,y:7));let line=input.draft
        _ = try engine.dispatch(command:"paint.colors.set",paramsJson:commandJSON(["foreground":[0.8,0.4,0.2,1]]));input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertEqual(input.draft,line);XCTAssertTrue(input.canConfirm)
        _ = try engine.dispatch(command:"layer.rename",paramsJson:commandJSON(["layer":["name":"Background"],"name":"new revision"]));let before=try engine.snapshotJson();XCTAssertTrue(input.confirm());try await finished(input);XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertTrue(input.error.isEmpty)
        input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:32,y:7));_ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertFalse(input.hasDraft);XCTAssertFalse(input.confirm())
    }
    @MainActor
    func testGuideValidationAndZeroCorrectionNeverWriteDocumentHistory()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson()
        for point in [NSPoint(x:Double.nan,y:1),NSPoint(x:1e13,y:1)]{input.down(point);XCTAssertFalse(input.hasDraft)}
        input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:2,y:4));XCTAssertFalse(input.canConfirm);XCTAssertTrue(input.confirm());XCTAssertEqual(try engine.snapshotJson(),before)
        input.cancel();input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:Double.infinity,y:7));XCTAssertNil(input.path());XCTAssertFalse(input.canConfirm);XCTAssertTrue(input.confirm());XCTAssertEqual(try engine.snapshotJson(),before)
        input.cancel();input.down(NSPoint(x:2,y:4));input.up(NSPoint(x:32,y:4));XCTAssertEqual(input.correction,0);XCTAssertTrue(input.confirm());try await finished(input);XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertFalse(input.hasDraft)
        let forward=StraightenLine(start:NSPoint(x:2,y:4),end:NSPoint(x:32,y:7)),reverse=StraightenLine(start:NSPoint(x:32,y:7),end:NSPoint(x:2,y:4));XCTAssertEqual(try XCTUnwrap(forward.correction(align:"horizontal")),try XCTUnwrap(reverse.correction(align:"horizontal")),accuracy:1e-10)
    }
}
