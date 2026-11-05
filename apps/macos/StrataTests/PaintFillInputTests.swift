import AppKit
import XCTest
@testable import Strata

final class PaintFillInputTests:XCTestCase{
    @MainActor
    private func configured(_ command:String)throws->(StrataEngine,CanvasPaintInput){
        let engine=StrataEngine(listener:EngineEvents{_ in})
        _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":16,"height":16,"bit_depth":16,"background":[32768,32769,50001,65535]]))
        let state=try EditorSnapshot.decode(engine.snapshotJson());let input=CanvasPaintInput(engine:engine)
        input.configure(CanvasPaintConfiguration(command:command,layer:state.activeLayer,revision:state.revision,foreground:[1,0,0,1],background:[0,0,1,1]))
        return(engine,input)
    }
    @MainActor private func finish(_ input:CanvasPaintInput)async throws{for _ in 0..<300{try await Task.sleep(for:.milliseconds(10));if !input.running{return}};XCTFail("Paint worker did not finish")}
    @MainActor
    func testEachGradientShapeCommitsOnceAfterDragAndCancelKeepsHistory()async throws{
        for shape in ["linear","radial","angle","reflected","diamond"]{
            let(engine,input)=try configured("paint.gradient");let state=try EditorSnapshot.decode(engine.snapshotJson())
            input.configure(CanvasPaintConfiguration(layer:state.activeLayer,revision:state.revision,shape:shape))
            input.down(NSPoint(x:3.5,y:3.5),background:false);input.drag(NSPoint(x:10.5,y:10.5))
            XCTAssertNotNil(input.path());XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,0)
            input.up(NSPoint(x:12.5,y:12.5));try await finish(input)
            XCTAssertNil(input.path());XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,1)
            input.down(NSPoint(x:3,y:3),background:false);input.cancel();XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,1)
        }
    }
    @MainActor
    func testBucketAndReadOnlySamplerHaveDifferentHistoryEffects()async throws{
        let(engine,bucket)=try configured("paint.bucket");bucket.down(NSPoint(x:2,y:2),background:false);try await finish(bucket)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,1)
        let state=try EditorSnapshot.decode(engine.snapshotJson());let sampler=CanvasPaintInput(engine:engine)
        sampler.configure(CanvasPaintConfiguration(command:"paint.sample_color",revision:state.revision))
        var value:[Double]=[];var background=false
        sampler.onSample={color,target,_ in value=color;background=target}
        sampler.down(NSPoint(x:2,y:2),background:true);try await finish(sampler)
        XCTAssertTrue(background);XCTAssertEqual(value.count,4);XCTAssertGreaterThan(value[0],0.99)
        XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).history.count,1)
    }
}
