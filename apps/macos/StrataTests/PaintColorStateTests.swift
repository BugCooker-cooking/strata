import AppKit
import XCTest
@testable import Strata

final class PaintColorStateTests:XCTestCase{
    @MainActor
    func testNativePickerSampleSwapAndResetShareCorePaletteAndPreserveDocument()async throws{
        let engine=StrataEngine(listener:EngineEvents{_ in});_ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"working_space":"display_p3","background":[10001,30003,50001,32769]]));let controller=MainWindowController(engine:engine);defer{controller.canvas.shutdown();controller.window?.orderOut(nil)};controller.refresh();let before=try EditorSnapshot.decode(engine.snapshotJson())
        for step in 0..<60{controller.workspace.setPaintComponents([Double(step)/100,0.2,0.3,0.5],background:false)}
        controller.workspace.setPaintComponents([0.8,0.6,0.4,1],background:true)
        var state=try EditorSnapshot.decode(engine.snapshotJson())
        for _ in 0..<300{state=try EditorSnapshot.decode(engine.snapshotJson());if state.paintColors?.foreground==[0.59,0.2,0.3,0.5]&&state.paintColors?.background==[0.8,0.6,0.4,1]{break};try await Task.sleep(for:.milliseconds(10))}
        XCTAssertEqual(state.paintColors?.foreground,[0.59,0.2,0.3,0.5]);XCTAssertEqual(state.paintColors?.background,[0.8,0.6,0.4,1]);XCTAssertEqual(state.revision,before.revision);XCTAssertEqual(state.history.count,before.history.count);XCTAssertEqual(state.dirty,before.dirty);XCTAssertFalse(controller.busy)
        controller.workspace.swapPaintColors();for _ in 0..<300{state=try EditorSnapshot.decode(engine.snapshotJson());if state.paintColors?.foreground==[0.8,0.6,0.4,1]{break};try await Task.sleep(for:.milliseconds(10))};XCTAssertEqual(state.paintColors?.foreground,[0.8,0.6,0.4,1]);controller.refresh();let color=try XCTUnwrap(controller.workspace.foregroundColor.usingColorSpace(controller.workspace.workingColorSpace));XCTAssertEqual(color.redComponent,0.8,accuracy:0.00001)
        controller.perform("paint.colors.reset",[:]);for _ in 0..<300{state=try EditorSnapshot.decode(engine.snapshotJson());if state.paintColors?.foreground==[0,0,0,1]{break};try await Task.sleep(for:.milliseconds(10))};XCTAssertEqual(state.paintColors?.foreground,[0,0,0,1]);XCTAssertEqual(state.paintColors?.background,[1,1,1,1]);XCTAssertEqual(state.revision,before.revision);XCTAssertNil(controller.window?.attachedSheet)
    }
}
