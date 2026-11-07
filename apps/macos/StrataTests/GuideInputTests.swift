import AppKit
import XCTest
@testable import Strata

final class GuideInputTests:XCTestCase{
    @MainActor func testToolSwitchRefreshesGuideHitRoutingImmediatelyWithoutAnotherDocumentEvent()throws{
        let(engine,_)=try configured();_ = try engine.dispatch(command:"view.guides.add",paramsJson:commandJSON(["name":"Line","axis":"vertical","position":10.5]));let controller=MainWindowController(engine:engine);controller.refresh()
        controller.workspace.selectTool(.text);XCTAssertNil(controller.canvas.hitGuide(NSPoint(x:10.5,y:4)))
        controller.workspace.selectTool(.move);XCTAssertEqual(controller.canvas.hitGuide(NSPoint(x:10.5,y:4))?.name,"Line")
        controller.workspace.selectTool(.brush);XCTAssertNil(controller.canvas.hitGuide(NSPoint(x:10.5,y:4)))
        controller.workspace.selectTool(.move);XCTAssertNotNil(controller.canvas.hitGuide(NSPoint(x:10.5,y:4)))
    }
    @MainActor private func configured()throws->(StrataEngine,CanvasGuideInput){
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":32,"height":24,"bit_depth":16]));let input=CanvasGuideInput(engine:engine);input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.onCommitted={[weak input]in if let state=try? EditorSnapshot.decode(engine.snapshotJson()){input?.bind(state)}};return(engine,input)
    }
    @MainActor private func idle(_ input:CanvasGuideInput)async throws{for _ in 0..<500{if !input.running{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("guide worker did not finish")}
    @MainActor func testRulerDraftAndCancelAreReadOnlyAndOneCommitKeepsFractionalCanvasCoordinate()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson();input.begin(axis:"vertical",position:-3.25);input.move(position:12.75);XCTAssertEqual(try engine.snapshotJson(),before);input.cancel();XCTAssertNil(input.draft);XCTAssertEqual(try engine.snapshotJson(),before)
        input.begin(axis:"horizontal",position:6.5);input.move(position:10.25);XCTAssertTrue(input.finish());try await idle(input);XCTAssertTrue(input.error.isEmpty,input.error);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.guides?.count,1);XCTAssertEqual(after.guides?.first?.position,10.25);XCTAssertEqual(after.history.last?.command,"view.guides.add")
        let id=try XCTUnwrap(after.guides?.first?.id);input.begin(axis:"horizontal",position:10.25,guide:id);input.move(position:-8.5);XCTAssertTrue(input.finish());try await idle(input);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).guides?.first?.id,id)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).guides?.first?.position,10.25)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).guides?.count,0)
    }
    @MainActor func testStaleLockRevisionAndNewDocumentNeverRetargetGuideDraft()async throws{
        for change in ["lock","revision","document"]{
            let(engine,input)=try configured();input.begin(axis:"vertical",position:4)
            if change=="lock"{_ = try engine.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["guides_locked":true]))}
            else if change=="revision"{_ = try engine.dispatch(command:"layer.rename",paramsJson:commandJSON(["layer":["name":"Background"],"name":"changed"]))}
            else{_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"discard_changes":true]))}
            let truth=try engine.snapshotJson();XCTAssertTrue(input.finish());try await idle(input);XCTAssertFalse(input.error.isEmpty);XCTAssertEqual(try engine.snapshotJson(),truth)
            input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertNil(input.draft)
        }
    }
    @MainActor func testSnapPointerClickDoesNotEditAndDragUsesSameOneStepSnapCommand()async throws{
        let(engine,_)=try configured();_ = try engine.dispatch(command:"paint.fill",paramsJson:commandJSON(["layer":["name":"Background"],"color":[0.2,0.4,0.8,1]]));_ = try engine.dispatch(command:"view.guides.add",paramsJson:commandJSON(["name":"X","axis":"vertical","position":10.5]));_ = try engine.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["snap_enabled":true,"snap_artboard":false,"snap_tolerance":6]))
        let input=CanvasSnapInput(engine:engine);input.bind(try EditorSnapshot.decode(engine.snapshotJson()));let original=try engine.snapshotJson();XCTAssertTrue(input.begin(at:NSPoint(x:4,y:4),canvasPixelsPerPoint:1));input.finish(at:NSPoint(x:4,y:4));XCTAssertEqual(try engine.snapshotJson(),original)
        XCTAssertTrue(input.begin(at:NSPoint(x:4,y:4),canvasPixelsPerPoint:1));input.move(to:NSPoint(x:13,y:4));input.finish(at:NSPoint(x:13,y:4))
        for _ in 0..<500{if !input.busy{break};try await Task.sleep(for:.milliseconds(10))};XCTAssertFalse(input.busy);XCTAssertTrue(input.error.isEmpty,input.error)
        let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.layers.first?.x,11);XCTAssertEqual(after.history.last?.command,"layer.snap_position")
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).layers.first?.x,0)
    }
}
