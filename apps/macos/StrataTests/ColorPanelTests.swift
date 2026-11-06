import XCTest
@testable import Strata

final class ColorPanelTests:XCTestCase{
    @MainActor
    private func finished(_ panel:ColorPanelModel,version:UInt64)async throws{for _ in 0..<300{if panel.state?.version==version{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("color panel read did not finish")}
    @MainActor
    private func describe(_ engine:StrataEngine)throws->PaintColorDescription{let json=try engine.dispatch(command:"paint.colors.describe",paramsJson:"{}");return try JSONDecoder().decode(PaintColorDescription.self,from:Data(json.utf8))}
    @MainActor
    func testAsyncPanelReadNeverPublishesOlderPaletteValuesAfterRapidUpdates()async throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});let panel=ColorPanelModel(engine:engine);panel.bind(try EditorSnapshot.decode(engine.snapshotJson()))
        for red in [0.1,0.2,0.3]{_ = try engine.dispatch(command:"paint.colors.set",paramsJson:commandJSON(["foreground":[red,0.4,0.6,0.5]]));panel.bind(try EditorSnapshot.decode(engine.snapshotJson()))}
        let snapshot=try EditorSnapshot.decode(engine.snapshotJson());try await finished(panel,version:try XCTUnwrap(snapshot.paintColors?.version));let state=try XCTUnwrap(panel.state);XCTAssertEqual(state.foreground.rgba,[0.3,0.4,0.6,0.5]);XCTAssertEqual(state.foreground.hex,"#4D669980");XCTAssertTrue(panel.error.isEmpty);XCTAssertNil(snapshot.documentId)
    }
    @MainActor
    func testRgbDraftUsesTheSelectedDocumentCodeDepthAndCoreParsesTheSameSubmittedPayload()throws{
        for depth:UInt8 in [8,16]{
            let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":3,"bit_depth":depth,"working_space":"display_p3"]));let before=try EditorSnapshot.decode(engine.snapshotJson());let state=try describe(engine);var draft=ColorPanelDraft(state:state,target:"foreground",model:"rgb",depth:depth);draft.values=depth==8 ? [51,102,153]:[10001,30003,50001];draft.alpha=50
            let input=try XCTUnwrap(draft.colorInput(depth:depth));XCTAssertEqual(input["model"] as? String,depth==8 ? "rgb8":"rgb16");_ = try engine.dispatch(command:"paint.colors.set_components",paramsJson:commandJSON(["target":draft.target,"color":input,"expected_version":state.version]));let actual=try describe(engine).foreground.rgba;let maxCode=depth==8 ? 255.0:65535.0
            for index in 0..<3{XCTAssertEqual(actual[index],draft.values[index]/maxCode,accuracy:1e-12)};XCTAssertEqual(actual[3],0.5);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.revision,before.revision);XCTAssertEqual(after.dirty,before.dirty);XCTAssertEqual(after.history.count,before.history.count)
        }
    }
    @MainActor
    func testHsbAndHexDraftsShareCoreConversionAndKeepTheOtherTarget()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});var state=try describe(engine);var draft=ColorPanelDraft(state:state,target:"foreground",model:"hsb",depth:8);draft.values=[120,100,100];draft.alpha=25
        _ = try engine.dispatch(command:"paint.colors.set_components",paramsJson:commandJSON(["target":draft.target,"color":try XCTUnwrap(draft.colorInput(depth:8)),"expected_version":state.version]));state=try describe(engine);XCTAssertEqual(state.foreground.rgba,[0,1,0,0.25]);XCTAssertEqual(state.foreground.hex,"#00FF0040")
        draft=ColorPanelDraft(state:state,target:"background",model:"hex",depth:16);draft.hex="  #Ff880080\n";_ = try engine.dispatch(command:"paint.colors.set_components",paramsJson:commandJSON(["target":draft.target,"color":try XCTUnwrap(draft.colorInput(depth:16)),"expected_version":state.version]));state=try describe(engine);XCTAssertEqual(state.foreground.rgba,[0,1,0,0.25]);XCTAssertEqual(state.background.rgba,[1,136.0/255,0,128.0/255]);XCTAssertEqual(state.background.hex,"#FF880080");XCTAssertEqual(state.background.rgb16,[65535,34952,0,32896])
    }
    @MainActor
    func testInvalidDraftsCannotGenerateAnExecutablePayload()throws{
        var draft=ColorPanelDraft();for invalid in [[Double.nan,0,0],[-1,0,0],[256,0,0],[0.5,0,0],[0,0]]{draft.values=invalid;XCTAssertNil(draft.colorInput(depth:8))}
        draft.values=[0,0,0];for alpha in [-1.0,101,Double.infinity]{draft.alpha=alpha;XCTAssertNil(draft.colorInput(depth:8))}
        draft.alpha=100;draft.model="hsb";for invalid in [[361.0,50,50],[0,101,50],[0,50,101]]{draft.values=invalid;XCTAssertNil(draft.colorInput(depth:8))}
        draft.model="hex";for invalid in ["#12345","##123456","#123456789","#gggggg"]{draft.hex=invalid;XCTAssertNil(draft.colorInput(depth:8))}
    }
    @MainActor
    func testFormattedPanelReadsAndWritesDoNotInvalidateAnOwnedCropDraft()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":16,"bit_depth":16,"background":[10001,30003,50001,32769]]));let before=try EditorSnapshot.decode(engine.snapshotJson());let token=try engine.beginCrop(documentId:try XCTUnwrap(before.documentId),revision:before.revision)
        _ = try describe(engine);_ = try engine.dispatch(command:"paint.colors.set_components",paramsJson:commandJSON(["target":"foreground","color":["model":"hex","value":"#33669980"]]));_ = try describe(engine);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).revision,before.revision)
        try engine.endCrop(token:token,paramsJson:commandJSON(["rect":["x":2,"y":3,"width":8,"height":6],"pixels":"preserve"]),cancel:false);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.frame?.width,8);XCTAssertEqual(after.historyCursor,before.historyCursor+1)
    }
}
