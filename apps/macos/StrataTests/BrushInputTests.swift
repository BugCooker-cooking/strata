import AppKit
import XCTest
@testable import Strata

final class BrushInputTests:XCTestCase{
    @MainActor
    private func configured(erase:Bool=false,target:String="pixels")throws->(StrataEngine,CanvasBrushInput){
        let engine=StrataEngine(listener:EngineEvents{_ in})
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":16,"bit_depth":16,"background":[20000,30000,40000,65535]]))
        if target=="mask"{_ = try engine.dispatch(command:"layer.mask.add",paramsJson:commandJSON(["layer":["name":"Background"],"source":"white"]))}
        let id=try XCTUnwrap(EditorSnapshot.decode(engine.snapshotJson()).activeLayer)
        let input=CanvasBrushInput(engine:engine)
        input.configure(CanvasBrushConfiguration(layer:id,erase:erase,target:target,size:4,color:[1,0,0,1],value:0))
        return (engine,input)
    }
    @MainActor
    private func finish(_ input:CanvasBrushInput)async throws{
        for _ in 0..<300{try await Task.sleep(for:.milliseconds(10));if !input.running{return}}
        XCTFail("Brush worker did not finish")
    }
    @MainActor
    func testBrushAndEraserStageInputThenCommitExactlyOneHistory()async throws{
        for erase in [false,true]{
            let (engine,input)=try configured(erase:erase)
            var failures:[String]=[];input.onError={failures.append($0.localizedDescription)}
            input.down(NSPoint(x:4.5,y:4.5),pressure:1)
            input.drag(NSPoint(x:8.5,y:8.5),pressure:0.5)
            XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
            XCTAssertNotNil(input.path())
            input.up(NSPoint(x:12.5,y:12.5),pressure:1);try await finish(input)
            let state=try EditorSnapshot.decode(engine.snapshotJson())
            XCTAssertEqual(state.history.count,1);XCTAssertEqual(state.history.last?.command,erase ? "paint.erase" : "paint.stroke")
            XCTAssertNil(input.path());XCTAssertTrue(failures.isEmpty,failures.joined(separator:"; "))
            _ = try engine.dispatch(command:"edit.undo",paramsJson:"{}")
            XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).historyCursor,0)
        }
    }
    @MainActor
    func testEscapeAndConfigurationChangeCancelUncommittedStroke()throws{
        let (engine,input)=try configured()
        input.down(NSPoint(x:4,y:4),pressure:1);input.drag(NSPoint(x:8,y:8),pressure:1);input.cancel()
        XCTAssertNil(input.path());XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
        input.down(NSPoint(x:4,y:4),pressure:1)
        let id=try XCTUnwrap(EditorSnapshot.decode(engine.snapshotJson()).activeLayer)
        input.configure(CanvasBrushConfiguration(layer:id,size:8))
        XCTAssertNil(input.path());XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
    }
    @MainActor
    func testMaskStrokeAndForegroundBackgroundSwapUseNativeSettings()async throws{
        let (engine,input)=try configured(target:"mask")
        input.down(NSPoint(x:8.5,y:8.5),pressure:1);input.up(NSPoint(x:8.5,y:8.5),pressure:1);try await finish(input)
        let state=try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(state.history.count,2);XCTAssertNotNil(state.layers.first?.mask)
        let workspace=WorkspaceModel();workspace.foregroundColor = .red;workspace.backgroundColor = .blue
        workspace.swapPaintColors();XCTAssertEqual(workspace.foregroundColor,.blue);XCTAssertEqual(workspace.backgroundColor,.red)
    }
}
