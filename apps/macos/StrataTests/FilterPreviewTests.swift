import Foundation
import XCTest
@testable import Strata

final class FilterPreviewTests:XCTestCase{
    @MainActor
    func testImageSizeSamplingRetainsNativeLayerIdsAndOneUndoStep()throws{
        for interpolation in ["nearest","bilinear","area"]{
            let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":12,"bit_depth":16,"background":[10001,30003,50001,32769]]));let before=try EditorSnapshot.decode(engine.snapshotJson())
            _ = try engine.dispatch(command:"image.resize",paramsJson:commandJSON(["width":13,"height":9,"interpolation":interpolation]));let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.width,13);XCTAssertEqual(after.frame?.height,9);XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.historyCursor,before.historyCursor+1);XCTAssertEqual(after.history.last?.command,"image.resize")
            _ = try engine.dispatch(command:"image.resize",paramsJson:commandJSON(["width":13,"height":9]));XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,after.historyCursor)
            _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.frame?.width,16);XCTAssertEqual(undone.frame?.height,12);XCTAssertEqual(undone.historyCursor,before.historyCursor)
        }
    }
    @MainActor
    func testImageSizeAspectConstraintRoundsFromTheOriginalRatioAndRejectsInvalidInput(){
        XCTAssertEqual(ImageSizePanel.linkedSize(512,from:1024,to:1536),768)
        XCTAssertEqual(ImageSizePanel.linkedSize(100,from:1536,to:1024),67)
        XCTAssertEqual(ImageSizePanel.linkedSize(1,from:1000,to:1),1)
        for invalid in [Double.nan,Double.infinity,0,-1,100001]{XCTAssertNil(ImageSizePanel.linkedSize(invalid,from:16,to:12))}
        XCTAssertNil(ImageSizePanel.linkedSize(5,from:0,to:12))
    }
    @MainActor
    func testArbitraryImageAngleExpandsNativeFrameAndUndoKeepsLayerIds()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":12,"bit_depth":16,"background":[10001,30003,50001,32769]]));let before=try EditorSnapshot.decode(engine.snapshotJson());_ = try engine.dispatch(command:"image.rotate",paramsJson:commandJSON(["angle_degrees":27,"interpolation":"bilinear"]));let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertGreaterThan(after.frame?.width ?? 0,16);XCTAssertGreaterThan(after.frame?.height ?? 0,12);XCTAssertEqual(after.frame?.x,0);XCTAssertEqual(after.frame?.y,0);XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.history.last?.command,"image.rotate");XCTAssertEqual(after.historyCursor,before.historyCursor+1);_ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.frame?.width,16);XCTAssertEqual(undone.frame?.height,12);XCTAssertEqual(undone.historyCursor,before.historyCursor)
    }
    @MainActor
    func testWholeImageOrientationKeepsNativeLayerStructureAndEachVariantUndoes()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":13,"height":9,"bit_depth":16,"background":[20000,30000,40000,32769]]));let before=try EditorSnapshot.decode(engine.snapshotJson())
        for operation in ["rotate_90_cw","rotate_90_ccw","rotate_180","flip_horizontal","flip_vertical"]{
            _ = try engine.dispatch(command:"image.orient",paramsJson:commandJSON(["operation":operation]));let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.width,operation.contains("90") ? 9:13);XCTAssertEqual(after.frame?.height,operation.contains("90") ? 13:9);XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.history.last?.command,"image.orient");XCTAssertEqual(after.historyCursor,before.historyCursor+1)
            _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.frame?.width,13);XCTAssertEqual(undone.frame?.height,9);XCTAssertEqual(undone.historyCursor,before.historyCursor)
        }
    }
    @MainActor
    func testCropAndRevealNativeCommandsKeepLayerPositionAndUndoToCleanGeometry()throws{
        let(engine,_)=try configured();let before=try EditorSnapshot.decode(engine.snapshotJson())
        _ = try engine.dispatch(command:"image.crop",paramsJson:commandJSON(["rect":["x":3,"y":2,"width":9,"height":8],"pixels":"preserve","aspect_ratio":[4,3]]))
        let cropped=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(cropped.frame?.x,3);XCTAssertEqual(cropped.frame?.y,2);XCTAssertEqual(cropped.frame?.width,8);XCTAssertEqual(cropped.frame?.height,6);XCTAssertEqual(cropped.layers[0].x,0);XCTAssertEqual(cropped.history.count,before.history.count+1)
        _ = try engine.dispatch(command:"image.reveal_all",paramsJson:"{}");let revealed=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(revealed.frame?.width,16);XCTAssertEqual(revealed.frame?.height,16);XCTAssertEqual(revealed.frame?.x,0)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");_ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,before.historyCursor)
    }
    @MainActor
    func testCanvasSizeNativeCommandKeepsSelectionValidAndHasOneUndoStep()throws{
        let(engine,_)=try configured();_ = try engine.dispatch(command:"select.all",paramsJson:"{}")
        let before=try EditorSnapshot.decode(engine.snapshotJson())
        _ = try engine.dispatch(command:"image.canvas_size",paramsJson:commandJSON(["width":21,"height":19,"anchor":"center"]))
        let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.x,-3);XCTAssertEqual(after.frame?.y,-2);XCTAssertEqual(after.frame?.width,21);XCTAssertEqual(after.history.count,before.history.count+1);XCTAssertEqual(after.history.last?.command,"image.canvas_size")
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.frame?.width,16);XCTAssertEqual(undone.frame?.x,0);XCTAssertEqual(undone.historyCursor,before.historyCursor)
    }
    @MainActor
    func testTransformCornerInputRejectsMissingExtraAndNonfiniteCoordinates(){
        XCTAssertEqual(TransformPanel.parseCorners("0,0; 16,0;16,16;0,16"),[[0,0],[16,0],[16,16],[0,16]])
        for invalid in ["0,x,0;16,0;16,16;0,16","0,;16,0;16,16;0,16","NaN,0;16,0;16,16;0,16","inf,0;16,0;16,16;0,16","0,0;16,0;16,16;0,16;","0,0;16,0;16,16","1000000000001,0;16,0;16,16;0,16"]{XCTAssertNil(TransformPanel.parseCorners(invalid),invalid)}
    }
    @MainActor
    private func configured()throws->(StrataEngine,TonalPreviewModel){
        let engine=StrataEngine(listener:EngineEvents{_ in})
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":16,"bit_depth":16,"background":[20000,30000,40000,32768]]))
        _ = try engine.dispatch(command:"paint.gradient",paramsJson:commandJSON(["layer":["name":"Background"],"start":[0.5,0.5],"end":[15.5,15.5],"stops":[["at":0,"color":[0,0,0,1]],["at":1,"color":[1,1,1,1]]]]))
        let preview=TonalPreviewModel(engine:engine);preview.bind(try EditorSnapshot.decode(engine.snapshotJson()));return(engine,preview)
    }
    @MainActor
    private func idle(_ preview:TonalPreviewModel)async throws{for _ in 0..<400{try await Task.sleep(for:.milliseconds(10));if !preview.busy && !preview.applying{return}};XCTFail("Filter preview worker did not finish")}
    @MainActor
    func testEachFilterPreviewsWithoutHistoryAndAppliesOnce()async throws{
        for (command,params) in [("filter.gaussian_blur",["sigma":1.2] as [String:Any]),("filter.unsharp_mask",["sigma":1.2,"amount":1.5]),("filter.add_noise",["amount":0.08,"seed":42]),("transform.apply",["scope":"whole_layer","geometry":["type":"components","translation":[0.5,0.25]]]),("transform.apply",["scope":"selection","geometry":["type":"components","translation":[4.5,0.25]]])]{
            let(engine,preview)=try configured();if params["scope"] as? String == "selection"{_ = try engine.dispatch(command:"select.marquee",paramsJson:commandJSON(["x":3,"y":3,"width":6,"height":6]));preview.bind(try EditorSnapshot.decode(engine.snapshotJson()))};let before=try engine.snapshotJson();let old=try EditorSnapshot.decode(before)
            preview.configureFilter(command);let parameters=try commandJSON(params);preview.setEnabled(true,settings:parameters);try await idle(preview)
            XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertTrue(preview.error.isEmpty,preview.error)
            preview.apply(parameters);try await idle(preview);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.history.count,old.history.count+1);XCTAssertEqual(after.history.last?.command,command);XCTAssertTrue(preview.error.isEmpty,preview.error)
            _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,old.historyCursor)
        }
    }
    @MainActor
    func testChangingModeAndContextCannotCommitAnOldPreview()async throws{
        let(engine,preview)=try configured();let before=try engine.snapshotJson()
        preview.configureFilter("filter.gaussian_blur");preview.setEnabled(true,settings:try commandJSON(["sigma":1]))
        preview.configureFilter("filter.add_noise");preview.setEnabled(true,settings:try commandJSON(["amount":0.1,"seed":42]));try await idle(preview)
        XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertTrue(preview.error.isEmpty,preview.error)
        preview.cancel();try await idle(preview);XCTAssertThrowsError(try engine.endFilterPreview(paramsJson:"{}",cancel:false))
        preview.setEnabled(true,settings:try commandJSON(["amount":0.08]));_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":4,"bit_depth":16,"discard_changes":true]));preview.bind(try EditorSnapshot.decode(engine.snapshotJson()));try await idle(preview)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
    }
    @MainActor
    func testCancellationBeforeApplyWorkerStartsCannotReviveTheFilter()async throws{
        let(engine,preview)=try configured();preview.configureFilter("filter.gaussian_blur");let parameters=try commandJSON(["sigma":1.2]);let before=try engine.snapshotJson();preview.setEnabled(true,settings:parameters);try await idle(preview)
        // Both actions occur before yielding the MainActor to the apply task.
        // A cancellation after a completed commit must keep that committed edit.
        preview.apply(parameters);preview.cancel();try await idle(preview)
        XCTAssertEqual(try engine.snapshotJson(),before);XCTAssertThrowsError(try engine.endFilterPreview(paramsJson:parameters,cancel:false));XCTAssertFalse(preview.enabled)
    }
}
