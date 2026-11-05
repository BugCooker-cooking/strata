import AppKit
import XCTest
@testable import Strata

final class ClipboardTests:XCTestCase{
    @MainActor
    func testNativeClipboardRoundtripUsesNamedPasteboardAndPreservesOrigin()async throws{
        let source=StrataEngine(listener:EngineEvents{_ in})
        _ = try source.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"background":[32768,32769,50001,65535]]))
        _ = try source.dispatch(command:"select.marquee",paramsJson:commandJSON(["x":1,"y":2,"width":3,"height":4]))
        let copy=try AIJSON.decode(source.dispatch(command:"edit.copy",paramsJson:"{}"))
        let resource=try XCTUnwrap(copy["clipboard_resource"].string)
        let transfer=try source.clipboardExport(resource:resource,turnId:nil)
        XCTAssertEqual(transfer.originX,1);XCTAssertEqual(transfer.originY,2)
        let pasteboard=NSPasteboard.withUniqueName();defer{pasteboard.releaseGlobally()}
        try NativeClipboard.write(transfer,pasteboard:pasteboard)
        try source.clipboardConfirmWrite(resource:resource,turnId:nil)
        let destination=StrataEngine(listener:EngineEvents{_ in})
        _ = try destination.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16]))
        let imported=try await NativeClipboard.read(engine:destination,turn:nil,pasteboard:pasteboard)
        _ = try destination.dispatch(command:"edit.paste_in_place",paramsJson:commandJSON(["resource":imported]))
        let state=try EditorSnapshot.decode(destination.snapshotJson())
        XCTAssertEqual(state.layers.last?.x,1);XCTAssertEqual(state.layers.last?.y,2)
        XCTAssertEqual(state.history.count,1)
        _ = try destination.dispatch(command:"edit.undo",paramsJson:"{}")
        XCTAssertEqual(try EditorSnapshot.decode(destination.snapshotJson()).layers.count,1)
    }
    @MainActor
    func testNamedClipboardWithoutImageDoesNotGrantDocumentAccess()async throws{
        let pasteboard=NSPasteboard.withUniqueName();defer{pasteboard.releaseGlobally()}
        pasteboard.setString("not an image",forType:.string)
        let engine=StrataEngine(listener:EngineEvents{_ in})
        do{_ = try await NativeClipboard.read(engine:engine,turn:nil,pasteboard:pasteboard);XCTFail("Text must not turn into image access.")}
        catch{XCTAssertFalse(error.localizedDescription.isEmpty)}
    }
}
