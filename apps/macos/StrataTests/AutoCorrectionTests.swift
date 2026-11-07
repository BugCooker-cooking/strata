import XCTest
@testable import Strata

final class AutoCorrectionTests:XCTestCase{
    @MainActor
    private func configured()throws->(StrataEngine,TonalPreviewModel){
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":12,"bit_depth":16,"working_space":"display_p3"]));_ = try engine.dispatch(command:"paint.gradient",paramsJson:commandJSON(["layer":["name":"Background"],"start":[0.5,0.5],"end":[15.5,11.5],"stops":[["at":0,"color":[0.2,0.3,0.4,1]],["at":1,"color":[0.7,0.9,0.8,1]]]]));let preview=TonalPreviewModel(engine:engine);preview.bind(try EditorSnapshot.decode(engine.snapshotJson()));return(engine,preview)
    }
    @MainActor
    private func idle(_ preview:TonalPreviewModel)async throws{for _ in 0..<400{try await Task.sleep(for:.milliseconds(10));if !preview.busy && !preview.applying{return}};XCTFail("automatic correction preview did not finish")}
    @MainActor
    func testSwitchingAutomaticMethodsKeepsTruthAndCancellingQueuedApplyCannotPublish()async throws{
        let(engine,preview)=try configured();let before=try engine.snapshotJson();preview.configureFilter("image.auto")
        for method in ["contrast","tone","color"]{
            let parameters=try commandJSON(["settings":["method":method,"clip_percent":0]]);preview.setEnabled(true,settings:parameters);try await idle(preview);XCTAssertTrue(preview.error.isEmpty,preview.error);XCTAssertEqual(try engine.snapshotJson(),before)
        }
        let parameters=try commandJSON(["settings":["method":"color","clip_percent":0,"neutralize_strength":0.7]])
        preview.update(parameters);try await idle(preview);preview.apply(parameters);preview.cancel();try await idle(preview);XCTAssertEqual(try engine.snapshotJson(),before)
        XCTAssertThrowsError(try engine.endFilterPreview(paramsJson:parameters,cancel:false))
    }
    @MainActor
    func testDirectNativeAutomaticCommandsHaveOneUndoAndNoopPreservesRevision()throws{
        for method in ["tone","contrast","color"]{
            let(engine,_)=try configured();let before=try EditorSnapshot.decode(engine.snapshotJson())
            let result=try AIJSON.decode(engine.dispatch(command:"image.auto",paramsJson:commandJSON(["layer":["name":"Background"],"settings":["method":method,"clip_percent":0]])));XCTAssertTrue(result["changed"].bool);XCTAssertEqual(result["method"].string,method)
            let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.bitDepth,before.bitDepth);XCTAssertEqual(after.profileName,before.profileName);XCTAssertEqual(after.historyCursor,before.historyCursor+1);XCTAssertEqual(after.history.last?.command,"image.auto")
            _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,before.historyCursor)
        }
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":3,"bit_depth":16,"background":[30003,30003,30003,32769]]));let before=try EditorSnapshot.decode(engine.snapshotJson());let result=try AIJSON.decode(engine.dispatch(command:"image.auto",paramsJson:commandJSON(["layer":["name":"Background"],"settings":["method":"color"]])));XCTAssertFalse(result["changed"].bool);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.revision,before.revision);XCTAssertEqual(after.historyCursor,before.historyCursor);XCTAssertEqual(after.dirty,before.dirty)
    }
}
