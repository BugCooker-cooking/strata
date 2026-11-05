import Foundation
import XCTest
@testable import Strata

final class TonalAdjustmentTests: XCTestCase {
    @MainActor
    func testEveryPointColorDraftKeepsStrictCompleteSettingsThroughNativeDispatch() throws {
        let engine=StrataEngine(listener:EngineEvents{_ in})
        for type in ["vibrance","white_balance","shadows_highlights","color_balance","black_white","photo_filter","channel_mixer","invert","posterize","threshold","gradient_map","selective_color","desaturate"]{
            _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":4,"height":4,"bit_depth":16,"discard_changes":true]))
            _ = try engine.dispatch(command:"layer.add_tonal_adjustment",paramsJson:commandJSON(["name":type,"adjustment":["type":type]]))
            let initial=try EditorSnapshot.decode(engine.snapshotJson()),layer=try XCTUnwrap(initial.layers.last)
            var draft=PointColorDraft(layer.kind?.adjustment)
            XCTAssertEqual(draft.type,type);XCTAssertEqual(layer.title,tonalTitle(type));XCTAssertTrue(layer.isAdjustment)
            switch type{
            case "vibrance":draft.vibrance=25;draft.saturation = -5
            case "white_balance":draft.temperature=20;draft.tintShift = -10;draft.preserveLuminance=false
            case "shadows_highlights":draft.shadowsAmount=35;draft.highlightsAmount=25;draft.shadowsWidth=0.5;draft.highlightsWidth=0.5;draft.localRadius=18
            case "color_balance":draft.balance[2]=[5,-5,10];draft.preserveLuminance=false
            case "black_white":draft.weights[5]=50;draft.tinted=true;draft.tint=[0.8,0.6,0.4];draft.tintAmount=30
            case "photo_filter":draft.color=[0.3,0.6,1];draft.density=45;draft.preserveLuminance=false
            case "channel_mixer":draft.matrix[1]=[0.2,0.7,0.1,0.03];draft.monochrome=true;draft.gray=[0.2,0.7,0.1,0]
            case "posterize":draft.levels=12
            case "threshold":draft.threshold=0.7
            case "gradient_map":draft.stops.insert(PointGradientStop(at:0.5,color:[0.6,0.3,0.8]),at:1);draft.reverse=true
            case "selective_color":draft.ranges[8]=[5,-5,10,20];draft.relative=false
            default:break
            }
            _ = try engine.dispatch(command:"adjust.tonal.set",paramsJson:commandJSON(["layer":["id":layer.id],"adjustment":draft.parameters]))
            let state=try EditorSnapshot.decode(engine.snapshotJson())
            XCTAssertEqual(PointColorDraft(state.layers.last?.kind?.adjustment),draft)
            XCTAssertEqual(state.history.count,2)
            _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}")
            XCTAssertEqual(PointColorDraft(try EditorSnapshot.decode(engine.snapshotJson()).layers.last?.kind?.adjustment),PointColorDraft(layer.kind?.adjustment))
        }
    }
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
