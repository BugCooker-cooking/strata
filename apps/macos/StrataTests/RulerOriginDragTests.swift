import AppKit
import XCTest
@testable import Strata

final class RulerOriginDragTests:XCTestCase{
    @MainActor private func configured()throws->(StrataEngine,CanvasRulerOriginInput){
        let e=StrataEngine(listener:EngineEvents{_ in});_ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":80,"height":40,"bit_depth":16]));_ = try e.dispatch(command:"view.guides.add",paramsJson:commandJSON(["name":"Absolute","axis":"vertical","position":17.5]));_ = try e.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["ruler_unit":"percent"]))
        let input=CanvasRulerOriginInput(engine:e);input.bind(try EditorSnapshot.decode(e.snapshotJson()));return(e,input)
    }
    @MainActor private func idle(_ input:CanvasRulerOriginInput)async throws{for _ in 0..<300{if !input.running{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("origin commit did not finish")}
    @MainActor func testFractionalAbsoluteOriginCommitsOnceWhileDraftCancelAndResetKeepGuidesHistoryAndPixels()async throws{
        let(e,input)=try configured();let before=try EditorSnapshot.decode(e.snapshotJson());XCTAssertTrue(input.begin(at:NSPoint(x:-12.5,y:8.25)));input.move(to:NSPoint(x:20.5,y:10.25));XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).presentation,before.presentation)
        input.cancel();XCTAssertNil(input.draft);XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).revision,before.revision)
        XCTAssertTrue(input.begin(at:.zero));input.move(to:NSPoint(x:20.5,y:10.25));XCTAssertTrue(input.finish(at:NSPoint(x:20.5,y:10.25)));XCTAssertTrue(input.running);try await idle(input);XCTAssertTrue(input.error.isEmpty,input.error)
        let moved=try EditorSnapshot.decode(e.snapshotJson());XCTAssertEqual(moved.presentation?.rulerOrigin,[20.5,10.25]);XCTAssertEqual(moved.presentation?.version,(before.presentation?.version ?? 0)+1);XCTAssertEqual(moved.presentation?.rulerUnit,"percent");XCTAssertEqual(moved.guides,before.guides);XCTAssertEqual(moved.historyCursor,before.historyCursor);XCTAssertEqual(moved.revision,before.revision);XCTAssertEqual(moved.dirty,before.dirty)
        input.bind(moved);input.reset();try await idle(input);XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).presentation?.rulerOrigin,[0,0])
        _ = try e.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(e.snapshotJson());XCTAssertEqual(undone.guides?.count,0);XCTAssertEqual(undone.presentation?.rulerOrigin,[0,0])
    }
    @MainActor func testOutsideInvalidAndStaleViewOrDocumentReleaseNeverRetargetsOrigin()async throws{
        for change in ["outside","nan","view","document"]{
            let(e,input)=try configured();XCTAssertTrue(input.begin(at:.zero));input.move(to:NSPoint(x:10,y:20))
            if change=="outside"{XCTAssertFalse(input.finish(at:nil))}
            else if change=="nan"{XCTAssertFalse(input.finish(at:NSPoint(x:CGFloat.nan,y:0)))}
            else{
                if change=="view"{_ = try e.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["ruler_origin":[7,9]]))}
                else{_ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":4,"bit_depth":8,"discard_changes":true]))}
                let truth=try EditorSnapshot.decode(e.snapshotJson());XCTAssertTrue(input.finish(at:NSPoint(x:10,y:20)));try await idle(input);XCTAssertFalse(input.error.isEmpty);XCTAssertEqual(try EditorSnapshot.decode(e.snapshotJson()).presentation,truth.presentation)
            }
            XCTAssertNil(input.draft)
        }
    }
    @MainActor func testCornerRoutesDragDoubleClickResetAndEscapeWithoutLaunchingAnotherWindow()throws{
        let corner=CanvasRulerCorner();var phases:[Int]=[],resets=0,cancels=0;corner.onPointer={phase,_ in phases.append(phase)};corner.onReset={resets+=1};corner.onCancel={cancels+=1}
        func mouse(_ type:NSEvent.EventType,_ count:Int)throws->NSEvent{try XCTUnwrap(NSEvent.mouseEvent(with:type,location:NSPoint(x:9,y:9),modifierFlags:[],timestamp:0,windowNumber:0,context:nil,eventNumber:1,clickCount:count,pressure:1))}
        corner.mouseDown(with:try mouse(.leftMouseDown,1));corner.mouseDragged(with:try mouse(.leftMouseDragged,1));corner.mouseUp(with:try mouse(.leftMouseUp,1));XCTAssertEqual(phases,[0,1,2])
        corner.mouseDown(with:try mouse(.leftMouseDown,2));XCTAssertEqual(resets,1);XCTAssertEqual(cancels,1)
        let escape=try XCTUnwrap(NSEvent.keyEvent(with:.keyDown,location:.zero,modifierFlags:[],timestamp:0,windowNumber:0,context:nil,characters:"\u{1b}",charactersIgnoringModifiers:"\u{1b}",isARepeat:false,keyCode:53));corner.keyDown(with:escape);XCTAssertEqual(cancels,2)
    }
    @MainActor func testToolChangeAndHiddenRulersClearPendingCornerDraftImmediately()throws{
        let(e,_)=try configured();let controller=MainWindowController(engine:e);controller.refresh();let input=controller.canvas.rulerOriginInput
        XCTAssertTrue(input.begin(at:NSPoint(x:7,y:9)));controller.workspace.selectTool(.brush);XCTAssertNil(input.draft)
        XCTAssertTrue(input.begin(at:NSPoint(x:7,y:9)));_ = try e.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["rulers_visible":false]));controller.refresh();XCTAssertNil(input.draft);XCTAssertFalse(input.begin(at:.zero))
    }
}
