import AppKit
import XCTest
@testable import Strata

final class EditableTextTests:XCTestCase{
    @MainActor
    func testRasterizationAndUndoRestoreTextEditingWithOrderedGeometry() async throws {
        let (engine, text) = try configured()
        text.content = "Raster native"
        text.fontSize = 24
        text.newText(at: NSPoint(x: 8, y: 40))
        text.apply()
        try await idle(text)
        let typeID = try XCTUnwrap(EditorSnapshot.decode(engine.snapshotJson()).activeLayer)
        _ = try engine.dispatch(command: "transform.apply", paramsJson: commandJSON([
            "layer": ["id":typeID], "scope":"whole_layer",
            "geometry": ["type":"affine", "values":[0,1,-1,0,120,0]], "interpolation":"nearest"
        ]))
        _ = try engine.dispatch(command: "image.crop", paramsJson: commandJSON([
            "rect":["x":16,"y":4,"width":140,"height":120], "pixels":"delete"
        ]))
        let before = try EditorSnapshot.decode(engine.snapshotJson())
        let layer = try XCTUnwrap(before.layers.last)
        let definition = try XCTUnwrap(layer.kind?.text)
        XCTAssertEqual(definition["operations"].array.count, 2)
        text.bind(before)
        XCTAssertFalse(text.creating)
        _ = try engine.dispatch(command: "layer.rasterize", paramsJson: commandJSON(["layer":["id":layer.id]]))
        let baked = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(baked.layers.last?.id, layer.id)
        XCTAssertEqual(baked.layers.last?.kind?.kind, "pixel")
        XCTAssertEqual(baked.historyCursor, before.historyCursor+1)
        text.bind(baked)
        XCTAssertTrue(text.creating)
        _ = try engine.dispatch(command: "edit.undo", paramsJson: "{}")
        let restored = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(restored.layers.last?.kind?.text, definition)
        text.bind(restored)
        XCTAssertFalse(text.creating)
        XCTAssertEqual(text.content, "Raster native")
        text.content = "Reedited source"
        text.apply()
        try await idle(text)
        let edited = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(edited.layers.last?.kind?.text?["operations"], definition["operations"])
        XCTAssertEqual(edited.layers.last?.kind?.text?["spec"]["text"].string, "Reedited source")
    }

    @MainActor
    func testPointerPlacementIsOverlayOnlyAndCannotRetargetAfterContextOrToolChanges()throws{
        let(engine,text)=try configured(),model=WorkspaceModel(),canvas=CanvasView(engine:engine)
        model.snapshot=try EditorSnapshot.decode(engine.snapshotJson());model.textEditing=text;model.tool = .text;canvas.configureText(model)
        var placements:[(NSPoint,CGRect?)]=[];canvas.onTextPlacement={placements.append(($0,$1))};let before=try engine.snapshotJson()
        canvas.beginTextPlacement(NSPoint(x:8,y:40));canvas.endTextPlacement(NSPoint(x:9,y:41));XCTAssertEqual(placements.count,1);XCTAssertEqual(placements[0].0,NSPoint(x:8,y:40));XCTAssertNil(placements[0].1);XCTAssertEqual(try engine.snapshotJson(),before)
        text.layout="paragraph";canvas.configureText(model);canvas.beginTextPlacement(NSPoint(x:30,y:40));canvas.updateTextPlacement(NSPoint(x:10,y:20));canvas.endTextPlacement(NSPoint(x:10,y:20));XCTAssertEqual(placements.last?.1,CGRect(x:10,y:20,width:20,height:20));XCTAssertEqual(try engine.snapshotJson(),before)
        canvas.beginTextPlacement(NSPoint(x:1,y:1));_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"discard_changes":true]));model.snapshot=try EditorSnapshot.decode(engine.snapshotJson());canvas.configureText(model);canvas.endTextPlacement(NSPoint(x:5,y:5));XCTAssertEqual(placements.count,2)
        canvas.beginTextPlacement(NSPoint(x:1,y:1));model.tool = .hand;canvas.configureText(model);canvas.endTextPlacement(NSPoint(x:5,y:5));XCTAssertEqual(placements.count,2)
    }
    @MainActor
    private func configured()throws->(StrataEngine,TextEditingModel){
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":200,"height":140,"bit_depth":16,"working_space":"display_p3"]));let text=TextEditingModel(engine:engine);text.bind(try EditorSnapshot.decode(engine.snapshotJson()));text.onCommitted={[weak text]in if let state=try? EditorSnapshot.decode(engine.snapshotJson()){text?.bind(state)}};return(engine,text)
    }
    @MainActor
    private func idle(_ text:TextEditingModel)async throws{for _ in 0..<500{if !text.running{return};try await Task.sleep(for:.milliseconds(10))};XCTFail("native text worker did not finish")}
    @MainActor
    func testCreateEditRevertAndOneUndoPreserveEditableSourceAndDepth()async throws{
        let(engine,text)=try configured();text.newText(at:NSPoint(x:8,y:40));text.content="中文 Strata";text.fontSize=24;text.red=20;text.green=40;text.blue=80;text.alpha=70;text.apply();try await idle(text);XCTAssertTrue(text.error.isEmpty,text.error)
        let before=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(before.bitDepth,16);XCTAssertEqual(before.history.count,1);XCTAssertEqual(before.layers.last?.kind?.kind,"text");XCTAssertEqual(before.layers.last?.kind?.text?["spec"]["text"].string,"中文 Strata");XCTAssertFalse(text.creating)
        text.content="changed";text.revert();XCTAssertEqual(text.content,"中文 Strata");text.content="编辑 Serif";text.font="serif";text.tracking=1;text.align="center";text.layout="paragraph";text.width=120;text.height=70;text.apply();try await idle(text);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.activeLayer,before.activeLayer);XCTAssertEqual(after.history.count,before.history.count+1);XCTAssertEqual(after.layers.last?.kind?.text?["spec"]["text"].string,"编辑 Serif");XCTAssertEqual(after.layers.last?.kind?.text?["spec"]["layout"]["width"].number,120)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let undone=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(undone.layers.last?.kind?.text?["spec"]["text"].string,"中文 Strata");XCTAssertEqual(undone.historyCursor,before.historyCursor)
    }
    @MainActor
    func testPlacementAndStrictParametersUseBaselineOrIntegerParagraphBox(){
        let engine=StrataEngine(listener:EngineEvents{_ in}),text=TextEditingModel(engine:engine);text.newText(at:NSPoint(x:-2.4,y:20.6));XCTAssertEqual(text.x,-2);XCTAssertEqual(text.y,21);XCTAssertEqual(text.layout,"point")
        text.newText(rect:CGRect(x:2.4,y:3.6,width:100.2,height:50.1));XCTAssertEqual(text.x,2);XCTAssertEqual(text.y,3);XCTAssertEqual(text.width,101);XCTAssertEqual(text.height,51);XCTAssertNotNil(text.parameters);text.width=10.5;XCTAssertNil(text.parameters);text.width=100;text.alpha=101;XCTAssertNil(text.parameters);text.alpha=100;text.fontSize = .nan;XCTAssertNil(text.parameters)
    }
    @MainActor
    func testStaleDraftCannotRebindAfterRevisionOrDocumentChanges()async throws{
        let(engine,text)=try configured();text.newText(at:NSPoint(x:8,y:40));text.content="stale draft";_ = try engine.dispatch(command:"layer.rename",paramsJson:commandJSON(["layer":["name":"Background"],"name":"new revision"]));let before=try engine.snapshotJson();text.apply();try await idle(text);XCTAssertFalse(text.error.isEmpty);XCTAssertEqual(try engine.snapshotJson(),before)
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"discard_changes":true]));let next=try engine.snapshotJson();text.apply();try await idle(text);XCTAssertEqual(try engine.snapshotJson(),next)
    }
    @MainActor
    func testIdenticalNativeApplyDoesNotCreateAnotherHistory()async throws{
        let(engine,text)=try configured();text.content="Noop 中文";text.newText(at:NSPoint(x:8,y:40));text.apply();try await idle(text);let before=try EditorSnapshot.decode(engine.snapshotJson());text.apply();try await idle(text);let after=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(after.revision,before.revision);XCTAssertEqual(after.historyCursor,before.historyCursor);XCTAssertEqual(after.dirty,before.dirty)
    }
    @MainActor
    func testCanvasTransformAndReeditKeepSourceAndUseSharedHistory()async throws{
        let(engine,text)=try configured();text.content="变换 Type";text.fontSize=24;text.alpha=0;text.newText(at:NSPoint(x:8,y:40));text.apply();try await idle(text)
        let before=try EditorSnapshot.decode(engine.snapshotJson()),input=CanvasTransformInput(engine:engine)
        XCTAssertTrue(before.layers.last?.canTransform(scope:"whole_layer")==true);XCTAssertFalse(before.layers.last?.canTransform(scope:"selection")==true)
        input.bind(before);input.configure(enabled:true);input.start()
        for _ in 0..<500{if !input.running{break};try await Task.sleep(for:.milliseconds(10))}
        XCTAssertTrue(input.canConfirm,input.error);let quad=try XCTUnwrap(input.draft),corner=quad.points[2]
        input.down(corner,hitTolerance:0.1);input.up(NSPoint(x:corner.x+20,y:corner.y+10));XCTAssertTrue(input.confirm())
        for _ in 0..<500{if !input.running{break};try await Task.sleep(for:.milliseconds(10))}
        XCTAssertFalse(input.running);XCTAssertTrue(input.error.isEmpty,input.error)
        let transformed=try EditorSnapshot.decode(engine.snapshotJson()),operations=try XCTUnwrap(transformed.layers.last?.kind?.text?["operations"].array)
        XCTAssertEqual(transformed.historyCursor,before.historyCursor+1);XCTAssertEqual(operations.count,1)
        text.bind(transformed);text.content="重编辑 Serif";text.font="serif";text.alpha=100;text.apply();try await idle(text);XCTAssertTrue(text.error.isEmpty,text.error)
        let edited=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(edited.layers.last?.kind?.kind,"text");XCTAssertEqual(edited.layers.last?.kind?.text?["spec"]["text"].string,"重编辑 Serif");XCTAssertEqual(edited.layers.last?.kind?.text?["operations"],transformed.layers.last?.kind?.text?["operations"])
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");let restored=try EditorSnapshot.decode(engine.snapshotJson());XCTAssertEqual(restored.layers.last?.kind?.text,transformed.layers.last?.kind?.text)
        _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}");XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).layers.last?.kind?.text,before.layers.last?.kind?.text)
    }
}
