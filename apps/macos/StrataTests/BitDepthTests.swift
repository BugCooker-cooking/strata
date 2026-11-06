import XCTest
@testable import Strata

final class BitDepthTests:XCTestCase{
    @MainActor
    func testWholeDocumentDepthChangesKeepNativeLayersMasksProfileAndOneUndo()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":12,"bit_depth":8,"working_space":"display_p3","resolution_dpi":300,"background":[100,120,140,200]]));_ = try engine.dispatch(command:"layer.mask.add",paramsJson:commandJSON(["layer":["name":"Background"],"source":"white"]));_ = try engine.dispatch(command:"layer.fill.solid",paramsJson:commandJSON(["name":"fill","color":[12,34,56,78]]));let before=try EditorSnapshot.decode(engine.snapshotJson());let icc=engine.workingProfileIcc()
        _ = try engine.dispatch(command:"image.convert_bit_depth",paramsJson:commandJSON(["bit_depth":16]));let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.bitDepth,16);XCTAssertEqual(after.resolutionDpi,before.resolutionDpi);XCTAssertEqual(after.layers.map(\.id),before.layers.map(\.id));XCTAssertEqual(after.layers[0].mask?.enabled,before.layers[0].mask?.enabled);XCTAssertEqual(after.historyCursor,before.historyCursor+1);XCTAssertEqual(after.history.last?.command,"image.convert_bit_depth");XCTAssertEqual(engine.workingProfileIcc(),icc)
        let repeated=try AIJSON.decode(engine.dispatch(command:"image.convert_bit_depth",paramsJson:commandJSON(["bit_depth":16])));XCTAssertFalse(repeated["changed"].bool);XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).revision,after.revision)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.bitDepth,8);XCTAssertEqual(undone.historyCursor,before.historyCursor);XCTAssertEqual(engine.workingProfileIcc(),icc)
    }
    @MainActor
    func testSixteenToEightUsesQuantizedCodesAndUndoRestoresNativeSamplePrecision()throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":3,"bit_depth":16,"background":[10001,30003,50001,32769]]));let before=try EditorSnapshot.decode(engine.snapshotJson())
        let sample:[String:Any]=["x":0,"y":0,"sample":"current_layer"]
        let original=try AIJSON.decode(engine.sampleColor(paramsJson:commandJSON(sample),revision:before.revision))["color"].array.compactMap(\.number)
        _ = try engine.dispatch(command:"image.convert_bit_depth",paramsJson:commandJSON(["bit_depth":8]));let after=try EditorSnapshot.decode(engine.snapshotJson());let codes=try AIJSON.decode(engine.sampleColor(paramsJson:commandJSON(sample),revision:after.revision))["color"].array.compactMap(\.number);XCTAssertEqual(codes,[39,117,195,128].map{Double($0)/255})
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());let restored=try AIJSON.decode(engine.sampleColor(paramsJson:commandJSON(sample),revision:undone.revision))["color"].array.compactMap(\.number);XCTAssertEqual(restored,original);XCTAssertEqual(undone.bitDepth,16)
        for value in [9,32]{XCTAssertThrowsError(try engine.dispatch(command:"image.convert_bit_depth",paramsJson:commandJSON(["bit_depth":value])))}
    }
}
