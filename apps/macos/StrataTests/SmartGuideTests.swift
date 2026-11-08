import AppKit
import XCTest
@testable import Strata

final class SmartGuideTests:XCTestCase{
    @MainActor private func configured()throws->(StrataEngine,CanvasSnapInput,String){
        let e=StrataEngine(listener:EngineEvents{_ in});_ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":32,"height":24,"bit_depth":16]))
        _ = try e.dispatch(command:"layer.create",paramsJson:commandJSON(["name":"Moving","fill":[10001,30003,50001,40001]]));let moving=try XCTUnwrap(try EditorSnapshot.decode(e.snapshotJson()).activeLayer)
        _ = try e.dispatch(command:"layer.create",paramsJson:commandJSON(["name":"Reference","fill":[50001,20003,10005,65535]]));_ = try e.dispatch(command:"layer.set_position",paramsJson:commandJSON(["layer":["name":"Reference"],"x":50,"y":30]))
        _ = try e.dispatch(command:"layer.select",paramsJson:commandJSON(["layer":["id":moving]]));_ = try e.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["snap_enabled":true,"snap_artboard":false,"snap_layers":true]))
        let input=CanvasSnapInput(engine:e);input.bind(try EditorSnapshot.decode(e.snapshotJson()));return(e,input,moving)
    }
    @MainActor private func idle(_ input:CanvasSnapInput)async throws{for _ in 0..<500{if !input.busy{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("smart guide worker did not finish")}
    @MainActor func testOverlayReadsExactValidatedReferenceAndApplyUsesSameCandidateWithOneUndo()async throws{
        let(e,input,moving)=try configured();let before=try EditorSnapshot.decode(e.snapshotJson());XCTAssertTrue(input.begin(at:.zero,canvasPixelsPerPoint:1));input.move(to:NSPoint(x:19,y:7));XCTAssertTrue(input.busy,"queued preview owns the slot before its task first runs");try await idle(input)
        let display=try XCTUnwrap(input.displayResult);XCTAssertEqual(display["x"].number,18);XCTAssertEqual(display["y"].number,6);XCTAssertEqual(display["snapped_x"]["target"].string,"layer_anchor");XCTAssertEqual(display["snapped_x"]["coordinate"].number,50);XCTAssertEqual(display["snapped_x"]["source_anchor"].string,"maximum");XCTAssertEqual(display["snapped_x"]["reference_anchor"].string,"minimum")
        XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).revision,before.revision)
        input.finish(at:NSPoint(x:19,y:7));XCTAssertNil(input.displayResult);try await idle(input);XCTAssertTrue(input.error.isEmpty,input.error)
        let after=try EditorSnapshot.decode(e.snapshotJson());XCTAssertEqual(after.layers.first{$0.id==moving}?.x,18);XCTAssertEqual(after.layers.first{$0.id==moving}?.y,6);XCTAssertEqual(after.historyCursor,before.historyCursor+1)
        _ = try e.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).layers.first{$0.id==moving}?.x,0)
    }
    @MainActor func testCancellationSettingsChangeAndInvalidReleaseClearOverlayWithoutHistory()async throws{
        let(e,input,_)=try configured();let before=try EditorSnapshot.decode(e.snapshotJson());XCTAssertTrue(input.begin(at:.zero,canvasPixelsPerPoint:1));input.move(to:NSPoint(x:19,y:7));try await idle(input);XCTAssertNotNil(input.displayResult);input.cancel();XCTAssertNil(input.displayResult);try await idle(input);XCTAssertEqual(try e.snapPreviewResult(),"null");XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).revision,before.revision)
        input.bind(try EditorSnapshot.decode(e.snapshotJson()));XCTAssertTrue(input.begin(at:.zero,canvasPixelsPerPoint:1));input.move(to:NSPoint(x:19,y:7));try await idle(input)
        _ = try e.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["snap_layers":false]));input.bind(try EditorSnapshot.decode(e.snapshotJson()));XCTAssertFalse(input.active);XCTAssertNil(input.displayResult);try await idle(input);XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).historyCursor,before.historyCursor)
        _ = try e.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["snap_layers":true]));input.bind(try EditorSnapshot.decode(e.snapshotJson()));XCTAssertTrue(input.begin(at:.zero,canvasPixelsPerPoint:1));input.move(to:NSPoint(x:19,y:7));try await idle(input);input.finish(at:NSPoint(x:CGFloat.nan,y:0));try await idle(input);XCTAssertNil(input.displayResult);XCTAssertEqual(try e.snapPreviewResult(),"null");XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).historyCursor,before.historyCursor)
    }
}
