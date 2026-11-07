import AppKit
import XCTest
@testable import Strata

final class CanvasTransformTests:XCTestCase{
    private let base=TransformQuad(rect:CGRect(x:0,y:0,width:16,height:12))
    private func assertQuad(_ actual:TransformQuad,_ expected:[[Double]],accuracy:Double=1e-9,file:StaticString=#filePath,line:UInt=#line){
        XCTAssertEqual(actual.points.count,expected.count,file:file,line:line)
        for(i,p)in actual.points.enumerated(){XCTAssertEqual(Double(p.x),expected[i][0],accuracy:accuracy,file:file,line:line);XCTAssertEqual(Double(p.y),expected[i][1],accuracy:accuracy,file:file,line:line)}
    }
    func testAllEightScaleHandlesHoldTheOppositeAnchorAndAllowExplicitFlip(){
        let cases:[(TransformHandle,NSPoint,[[Double]])]=[
            (.topLeft,NSPoint(x:-4,y:-3),[[-4,-3],[16,-3],[16,12],[-4,12]]),
            (.top,NSPoint(x:8,y:-3),[[0,-3],[16,-3],[16,12],[0,12]]),
            (.topRight,NSPoint(x:20,y:-3),[[0,-3],[20,-3],[20,12],[0,12]]),
            (.right,NSPoint(x:20,y:6),[[0,0],[20,0],[20,12],[0,12]]),
            (.bottomRight,NSPoint(x:20,y:15),[[0,0],[20,0],[20,15],[0,15]]),
            (.bottom,NSPoint(x:8,y:15),[[0,0],[16,0],[16,15],[0,15]]),
            (.bottomLeft,NSPoint(x:-4,y:15),[[-4,0],[16,0],[16,15],[-4,15]]),
            (.left,NSPoint(x:-4,y:6),[[-4,0],[16,0],[16,12],[-4,12]])]
        for(handle,point,expected)in cases{let quad=base.resized(handle:handle,to:point,uniform:false,centered:false);XCTAssertTrue(quad.valid);assertQuad(quad,expected)}
        let flipped=base.resized(handle:.bottomRight,to:NSPoint(x:-16,y:12),uniform:false,centered:false);XCTAssertTrue(flipped.valid);assertQuad(flipped,[[0,0],[-16,0],[-16,12],[0,12]])
    }
    func testUniformCenteredAndRotatedLocalAxesHaveIndependentReferences(){
        let centered=base.resized(handle:.bottomRight,to:NSPoint(x:24,y:18),uniform:true,centered:true);assertQuad(centered,[[-8,-6],[24,-6],[24,18],[-8,18]])
        let rotated=base.rotated(.pi/2,around:base.center);assertQuad(rotated,[[14,-2],[14,14],[2,14],[2,-2]])
        let resized=rotated.resized(handle:.bottomRight,to:NSPoint(x:-10,y:30),uniform:false,centered:false);XCTAssertTrue(resized.valid);assertQuad(resized,[[14,-2],[14,30],[-10,30],[-10,-2]])
    }
    func testSkewDistortAndPairedPerspectiveCornersAndInvalidQuads(){
        assertQuad(base.skewed(handle:.top,delta:NSPoint(x:3,y:9)),[[3,0],[19,0],[16,12],[0,12]])
        assertQuad(base.distorted(handle:.topLeft,delta:NSPoint(x:2,y:1)),[[2,1],[16,0],[16,12],[0,12]])
        assertQuad(base.perspective(handle:.topLeft,delta:NSPoint(x:2,y:0)),[[2,0],[14,0],[16,12],[0,12]])
        assertQuad(base.perspective(handle:.bottomRight,delta:NSPoint(x:0,y:2)),[[0,0],[16,-2],[16,14],[0,12]])
        XCTAssertFalse(base.distorted(handle:.topLeft,delta:NSPoint(x:20,y:20)).valid)
        XCTAssertFalse(TransformQuad([.zero,.zero,.zero,.zero]).valid)
        XCTAssertFalse(TransformQuad([.zero,NSPoint(x:CGFloat.nan,y:0),NSPoint(x:1,y:1),NSPoint(x:0,y:1)]).valid)
        XCTAssertTrue(base.contains(NSPoint(x:8,y:6)));XCTAssertFalse(base.contains(NSPoint(x:20,y:6)))
    }
    @MainActor
    private func configured(scope:String="whole_layer")throws->(StrataEngine,CanvasTransformInput){
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":12,"bit_depth":16,"background":[12001,30003,50001,32769]]))
        let input=CanvasTransformInput(engine:engine);input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.configure(enabled:true,scope:scope);return(engine,input)
    }
    @MainActor
    private func idle(_ input:CanvasTransformInput)async throws{for _ in 0..<500{if !input.running{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("canvas transform worker did not finish")}
    @MainActor
    func testPointerDraftKeepsTruthAndShiftOptionScaleUsesOneUndo()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson();input.start();try await idle(input);XCTAssertTrue(input.error.isEmpty,input.error);XCTAssertEqual(try engine.snapshotJson(),before)
        input.down(NSPoint(x:16,y:12),hitTolerance:0.1);input.up(NSPoint(x:24,y:18),modifiers:[.shift,.option]);assertQuad(try XCTUnwrap(input.draft),[[-8,-6],[24,-6],[24,18],[-8,18]]);XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertNotNil(input.path())
        XCTAssertTrue(input.confirm());try await idle(input);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.history.count,1);XCTAssertEqual(after.history.last?.command,"transform.apply");XCTAssertNil(input.draft);XCTAssertTrue(input.error.isEmpty,input.error)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,0)
    }
    @MainActor
    func testHandleGrabOffsetShiftMoveNudgeAndInvalidDragCannotCreateHistory()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson();input.start();try await idle(input)
        input.down(NSPoint(x:16.03,y:12.04),hitTolerance:0.1);input.up(NSPoint(x:20.03,y:15.04));assertQuad(try XCTUnwrap(input.draft),[[0,0],[20,0],[20,15],[0,15]])
        input.down(NSPoint(x:10,y:7),hitTolerance:0.1);input.up(NSPoint(x:13,y:8),modifiers:.shift);assertQuad(try XCTUnwrap(input.draft),[[3,0],[23,0],[23,15],[3,15]])
        XCTAssertTrue(input.nudge(x:0,y:10));assertQuad(try XCTUnwrap(input.draft),[[3,10],[23,10],[23,25],[3,25]])
        input.configure(enabled:true,mode:"distort");let valid=try XCTUnwrap(input.draft);input.down(valid.points[0],hitTolerance:0.1);input.up(NSPoint(x:40,y:40));XCTAssertEqual(input.draft,valid);XCTAssertFalse(input.error.isEmpty);XCTAssertEqual(try engine.snapshotJson(),before)
        input.cancel();XCTAssertNil(input.path());XCTAssertFalse(input.confirm());XCTAssertEqual(try engine.snapshotJson(),before)
    }
    @MainActor
    func testRotationHandleSnapsAndNoopEnterKeepsRevisionAndHistory()async throws{
        let(engine,input)=try configured();let before=try EditorSnapshot.decode(engine.snapshotJson());input.start();try await idle(input);XCTAssertTrue(input.confirm());try await idle(input)
        let identity=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(identity.revision,before.revision);XCTAssertEqual(identity.history.count,before.history.count);XCTAssertEqual(identity.dirty,before.dirty)
        input.bind(identity);input.start();try await idle(input);let quad=try XCTUnwrap(input.draft);input.down(quad.rotationHandle(distance:0.3),hitTolerance:0.1);input.up(NSPoint(x:20,y:6),modifiers:.shift);assertQuad(try XCTUnwrap(input.draft),[[14,-2],[14,14],[2,14],[2,-2]])
    }
    @MainActor
    func testCancelledQueuedStartOrApplyAndChangedLayerDocumentOrToolCannotRetarget()async throws{
        let(engine,input)=try configured();let before=try engine.snapshotJson();input.start();input.cancel();try await idle(input);XCTAssertFalse(input.hasDraft);XCTAssertEqual(try engine.snapshotJson(),before)
        input.start();try await idle(input);XCTAssertTrue(input.nudge(x:2,y:3));XCTAssertTrue(input.confirm());input.cancel();try await idle(input);XCTAssertEqual(try engine.snapshotJson(),before)
        input.start();try await idle(input);_ = try engine.dispatch(command:"layer.create",paramsJson:commandJSON(["name":"other"]));input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertFalse(input.hasDraft);XCTAssertFalse(input.confirm())
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":4,"bit_depth":16,"discard_changes":true]));input.bind(try EditorSnapshot.decode(engine.snapshotJson()));XCTAssertFalse(input.hasDraft)
        input.configure(enabled:false);input.start();XCTAssertFalse(input.running);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
    }
    @MainActor
    func testSelectedPixelsModeUsesSameCornerCommandAndPreservesDocumentGeometry()async throws{
        let(engine,input)=try configured(scope:"selection");_ = try engine.dispatch(command:"select.marquee",paramsJson:commandJSON(["x":3,"y":2,"width":7,"height":6]));_ = try engine.dispatch(command:"select.feather",paramsJson:commandJSON(["radius":1]));let before=try EditorSnapshot.decode(engine.snapshotJson());input.bind(before);input.start();try await idle(input);XCTAssertTrue(input.canConfirm,input.error)
        XCTAssertTrue(input.nudge(x:2,y:1));let params=try XCTUnwrap(input.parameters);XCTAssertEqual(params["scope"] as? String,"selection");XCTAssertTrue(input.confirm());try await idle(input);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.width,before.frame?.width);XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.bitDepth,16);XCTAssertEqual(after.historyCursor,before.historyCursor+1)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,before.historyCursor)
    }
}
