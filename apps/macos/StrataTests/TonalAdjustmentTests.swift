import Foundation
import XCTest
@testable import Strata

final class TonalAdjustmentTests: XCTestCase {
    func testDraftRetainsEveryChannelAndSerializesStrictTonalSettings() throws {
        let input = """
        {"type":"levels","master":{"input_black":0.1,"input_white":0.9,"gamma":1.2,"output_black":0.02,"output_white":0.98},"channels":[{"input_black":0.03,"input_white":0.8,"gamma":0.7,"output_black":0.1,"output_white":0.7},{"gamma":2.2},{}]}
        """
        var draft = TonalDraft(try AIJSON.decode(input))
        XCTAssertEqual(draft.levels[0].gamma,1.2);XCTAssertEqual(draft.levels[1].outputWhite,0.7)
        XCTAssertEqual(draft.levels[2].gamma,2.2);XCTAssertEqual(draft.levels[3].inputWhite,1)
        draft.levels[1].gamma=1.8
        let restored=TonalDraft(try AIJSON.decode(commandJSON(draft.parameters)))
        XCTAssertEqual(restored,draft)
    }
    func testCurvePreviewUsesCoreInterpolationAtAllControlKnots() throws {
        let values=try tonalCurvePreview(pointsJson:"[[0,0],[0.25,0.7],[0.5,0.1],[0.75,0.8],[1,1]]")
        XCTAssertEqual(values.count,257);XCTAssertEqual(values[64],0.7,accuracy:0.00001);XCTAssertEqual(values[128],0.1,accuracy:0.00001)
        XCTAssertThrowsError(try tonalCurvePreview(pointsJson:"[[0,0],[0,1],[1,1]]"))
    }
    @MainActor
    func testDraftCommitCreatesOneHistoryAndUndoRestoresPriorCurveAndMask() throws {
        let engine=StrataEngine(listener:EngineEvents{_ in})
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16]))
        _ = try engine.dispatch(command:"select.marquee",paramsJson:commandJSON(["x":1,"y":2,"width":4,"height":3]))
        _ = try engine.dispatch(command:"layer.add_tonal_adjustment",paramsJson:commandJSON(["name":"曲线","adjustment":["type":"curves"]]))
        let state=try EditorSnapshot.decode(engine.snapshotJson()),layer=try XCTUnwrap(state.layers.last)
        XCTAssertTrue(layer.isAdjustment);XCTAssertEqual(layer.title,"曲线");XCTAssertNotNil(layer.mask)
        var draft=TonalDraft(layer.kind?.adjustment)
        draft.curves[2]=[TonalCurvePoint(0,0),TonalCurvePoint(0.5,0.7),TonalCurvePoint(1,1)]
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,2)
        _ = try engine.dispatch(command:"adjust.tonal.set",paramsJson:commandJSON(["layer":["id":layer.id],"adjustment":draft.parameters]))
        let adjusted=try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(adjusted.history.count,3);XCTAssertEqual(TonalDraft(adjusted.layers.last?.kind?.adjustment),draft)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}")
        let restored=try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(TonalDraft(restored.layers.last?.kind?.adjustment),TonalDraft(layer.kind?.adjustment))
        XCTAssertNotNil(restored.layers.last?.mask)
    }
}
