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
    @MainActor
    func testAllEightHandlesResizeOppositeEdgesAndKeepDraftOutOfHistory()throws{
        let(engine,input)=try configured();input.configure(enabled:true,ratio:"free");let before=try engine.snapshotJson()
        let cases:[(CropHandle,NSPoint,CGRect)]=[
            (.topLeft,NSPoint(x:0,y:1),CGRect(x:0,y:1,width:14,height:11)),
            (.top,NSPoint(x:8,y:1),CGRect(x:2,y:1,width:12,height:11)),
            (.topRight,NSPoint(x:16,y:1),CGRect(x:2,y:1,width:14,height:11)),
            (.left,NSPoint(x:0,y:7),CGRect(x:0,y:3,width:14,height:9)),
            (.right,NSPoint(x:16,y:7),CGRect(x:2,y:3,width:14,height:9)),
            (.bottomLeft,NSPoint(x:0,y:14),CGRect(x:0,y:3,width:14,height:11)),
            (.bottom,NSPoint(x:8,y:14),CGRect(x:2,y:3,width:12,height:11)),
            (.bottomRight,NSPoint(x:16,y:14),CGRect(x:2,y:3,width:14,height:11))]
        for(handle,point,expected)in cases{
            input.cancel();input.down(NSPoint(x:2,y:3));input.up(NSPoint(x:14,y:12));let rect=try XCTUnwrap(input.draft)
            input.down(handle.position(in:rect),hitTolerance:0.5);input.up(point);XCTAssertEqual(input.draft,expected,"\(handle)");XCTAssertEqual(try engine.snapshotJson(),before)
        }
    }
    @MainActor
    func testMoveAndRatioResizeCommitDisplayedRectAndKeepInitialRevisionBinding()async throws{
        let(engine,input)=try configured();input.configure(enabled:true,ratio:"4:3");input.down(NSPoint(x:2,y:3));input.up(NSPoint(x:14,y:12))
        input.down(NSPoint(x:8,y:7),hitTolerance:0.5);input.up(NSPoint(x:13,y:3));XCTAssertEqual(input.draft,CGRect(x:7,y:-1,width:12,height:9))
        input.down(NSPoint(x:19,y:8),hitTolerance:0.5);input.up(NSPoint(x:23,y:11));XCTAssertEqual(input.draft,CGRect(x:7,y:-1,width:16,height:12))
        XCTAssertTrue(input.confirm());try await finished(input);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.x,7);XCTAssertEqual(after.frame?.y,-1);XCTAssertEqual(after.frame?.width,16);XCTAssertEqual(after.frame?.height,12);XCTAssertEqual(after.history.count,1)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.down(NSPoint(x:2,y:3));input.up(NSPoint(x:14,y:12))
        input.down(NSPoint(x:2,y:3),hitTolerance:0.5);input.up(NSPoint(x:-2,y:0));XCTAssertEqual(input.draft,CGRect(x:-2,y:0,width:16,height:12))
        _ = try engine.dispatch(command:"layer.rename",paramsJson:commandJSON(["layer":["name":"Background"],"name":"new revision"]))
        XCTAssertTrue(input.confirm());try await finished(input);let snapshot=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(snapshot.frame?.width,16);XCTAssertEqual(snapshot.frame?.x,0);XCTAssertEqual(snapshot.history.last?.command,"layer.rename")
    }
    @MainActor
    func testExplicitDeleteModeUsesSameOwnedGestureAndExpansionCannotRestoreDeletedPixels()async throws{
        let(engine,input)=try configured();let before=try EditorSnapshot.decode(engine.snapshotJson());input.configure(enabled:true,ratio:"free",deletePixels:true);input.down(NSPoint(x:3,y:2));input.up(NSPoint(x:11,y:10));XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,before.history.count);XCTAssertTrue(input.confirm());try await finished(input);let cropped=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(cropped.frame?.x,3);XCTAssertEqual(cropped.frame?.width,8);XCTAssertEqual(cropped.history.count,before.history.count+1)
        _ = try engine.dispatch(command:"image.canvas_size",paramsJson:commandJSON(["width":16,"height":16,"anchor":"center"]));let expanded=try EditorSnapshot.decode(engine.snapshotJson());let outside=try AIJSON.decode(engine.sampleColor(paramsJson:commandJSON(["x":0,"y":0,"sample":"current_layer"]),revision:expanded.revision))["color"].array.compactMap(\.number);XCTAssertEqual(outside,[0,0,0,0]);_ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");_ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,before.historyCursor)
    }
    @MainActor
    func testNativeTrimPreservesPixelLayerAndTightRepeatDoesNotMakeHistory()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":16,"bit_depth":16]));_ = try engine.dispatch(command:"select.marquee",paramsJson:commandJSON(["x":3,"y":2,"width":8,"height":9]));_ = try engine.dispatch(command:"paint.fill",paramsJson:commandJSON(["layer":["name":"Background"],"color":[0.8,0.2,0.1,0.5]]));_ = try engine.dispatch(command:"select.none",paramsJson:"{}");let before=try EditorSnapshot.decode(engine.snapshotJson())
        _ = try engine.dispatch(command:"image.trim",paramsJson:"{}");let trimmed=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(trimmed.frame?.x,3);XCTAssertEqual(trimmed.frame?.y,2);XCTAssertEqual(trimmed.frame?.width,8);XCTAssertEqual(trimmed.frame?.height,9);XCTAssertEqual(trimmed.layers[0].x,0);XCTAssertEqual(trimmed.history.count,before.history.count+1)
        let repeated=try AIJSON.decode(engine.dispatch(command:"image.trim",paramsJson:"{}"));XCTAssertFalse(repeated["changed"].bool);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.revision,trimmed.revision);XCTAssertEqual(after.history.count,trimmed.history.count)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).frame?.width,16)
    }
}
