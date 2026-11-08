import AppKit
import XCTest
@testable import Strata

final class RulerUnitsTests:XCTestCase{
    @MainActor private func engine()throws->StrataEngine{
        let engine=StrataEngine(listener:EngineEvents{_ in})
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":3000,"height":1500,"resolution_dpi":300,"bit_depth":16]))
        return engine
    }
    @MainActor func testDisplayUsesCorePhysicalMetricsAndOriginWhileGuidesAndHistoryRemainPixels()throws{
        let engine=try engine()
        _ = try engine.dispatch(command:"view.guides.add",paramsJson:commandJSON(["name":"X","axis":"vertical","position":400]))
        let before=try EditorSnapshot.decode(engine.snapshotJson())
        _ = try engine.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["ruler_unit":"millimeters","ruler_origin":[100,200]]))
        let state=try EditorSnapshot.decode(engine.snapshotJson()),metrics=try XCTUnwrap(state.rulerMetrics),point=try XCTUnwrap(metrics.rulerPoint(NSPoint(x:400,y:800)))
        XCTAssertEqual(point.x,25.4,accuracy:1e-10);XCTAssertEqual(point.y,50.8,accuracy:1e-10);XCTAssertEqual(metrics.symbol,"mm");XCTAssertEqual(state.revision,before.revision);XCTAssertEqual(state.historyCursor,before.historyCursor);XCTAssertEqual(state.guides,before.guides)
        let command=try AIJSON.decode(engine.dispatch(command:"view.rulers.convert",paramsJson:commandJSON(["coordinates":[400,800]])))
        XCTAssertEqual(try XCTUnwrap(command["ruler"].array.first?.number),Double(point.x),accuracy:1e-12)
        let model=WorkspaceModel();model.snapshot=state;let canvas=CanvasView(engine:engine),chrome=CanvasChromeView(canvas:canvas,model:model)
        canvas.onViewChanged?(ArtboardOverlay(x:20,y:40,width:3000,height:1500,name:"test"),2,2)
        let rulers=chrome.subviews.compactMap{$0 as? CanvasRuler};let horizontal=try XCTUnwrap(rulers.first{!$0.vertical}),vertical=try XCTUnwrap(rulers.first{$0.vertical})
        // 2 physical pixels per canvas pixel on a2x screen ->1pt per px.
        // Given image screen origin10/20pt and ruler canvas zero100/200px,
        // actual ruler zero must be at110/220pt;25.4mm spans300pt.
        XCTAssertEqual(horizontal.origin,110,accuracy:1e-9);XCTAssertEqual(vertical.origin,220,accuracy:1e-9)
        XCTAssertEqual(horizontal.unitScale*25.4,300,accuracy:1e-9);XCTAssertEqual(vertical.unitScale*25.4,300,accuracy:1e-9)
        XCTAssertEqual(horizontal.unitSymbol,"mm");XCTAssertEqual(vertical.unitSymbol,"mm")
    }
    @MainActor func testPercentageMetricsTrackNewFrameAndPresentationChangeCancelsPendingGuide()throws{
        let engine=try engine();let input=CanvasGuideInput(engine:engine)
        input.bind(try EditorSnapshot.decode(engine.snapshotJson()));input.begin(axis:"vertical",position:200);XCTAssertNotNil(input.draft)
        _ = try engine.dispatch(command:"view.presentation.set",paramsJson:commandJSON(["ruler_unit":"percent","ruler_origin":[100,200]]));let state=try EditorSnapshot.decode(engine.snapshotJson());input.bind(state);XCTAssertNil(input.draft)
        let metrics=try XCTUnwrap(state.rulerMetrics),point=try XCTUnwrap(metrics.rulerPoint(NSPoint(x:3100,y:1700)))
        XCTAssertEqual(point.x,100,accuracy:1e-10);XCTAssertEqual(point.y,100,accuracy:1e-10)
        _ = try engine.dispatch(command:"image.crop",paramsJson:commandJSON(["rect":["x":100,"y":200,"width":600,"height":300],"pixels":"preserve"]))
        let cropped=try EditorSnapshot.decode(engine.snapshotJson()),small=try XCTUnwrap(cropped.rulerMetrics),value=try XCTUnwrap(small.rulerPoint(NSPoint(x:700,y:500)))
        XCTAssertEqual(value.x,100,accuracy:1e-10);XCTAssertEqual(value.y,100,accuracy:1e-10);XCTAssertEqual(small.symbol,"%")
        XCTAssertEqual(cropped.presentation?.rulerOrigin,[100,200]);XCTAssertEqual(cropped.presentation?.version,state.presentation?.version)
    }
}
