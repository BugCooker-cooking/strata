import AppKit
import XCTest
@testable import Strata

@MainActor
final class QuickExportTests:XCTestCase{
    private func engine()->StrataEngine{StrataEngine(listener:EngineEvents{_ in})}
    private func set(_ e:StrataEngine,_ options:AIJSON)throws{_ = try e.dispatch(command:"file.export_preset.set",paramsJson:AIJSON.object(["options":options]).encoded())}
    func testSharedPresetUsesExactVersionsAcrossIndependentEnginesAndDoesNotDirtyDocument()throws{
        let a=engine(),b=engine(),prefs=QuickExportPreferences();let initial=try prefs.preset(a);defer{try? set(a,initial.options)}
        _ = try a.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16]));let before=try EditorSnapshot.decode(a.snapshotJson())
        try set(a,AIJSON.decode("{\"format\":\"jpeg\",\"quality\":83,\"background\":[12,34,56],\"allow_quantization\":true}"))
        let one=try prefs.preset(a),two=try prefs.preset(b);XCTAssertEqual(one.version,two.version);XCTAssertEqual(one.options,two.options)
        let after=try EditorSnapshot.decode(a.snapshotJson());XCTAssertEqual(after.revision,before.revision);XCTAssertEqual(after.historyCursor,before.historyCursor);XCTAssertEqual(after.dirty,before.dirty)
        XCTAssertThrowsError(try b.dispatch(command:"file.export_preset.set",paramsJson:commandJSON(["options":["format":"png"],"expected_version":initial.version])))
    }
    func testPersistentOptionsRestoreWithoutAnyPathsTokensOrOverwriteAndCorruptDataIsAtomic()throws{
        let suite="dev.strata.quick-export-tests.\(UUID().uuidString)",defaults=try XCTUnwrap(UserDefaults(suiteName:suite)),prefs=QuickExportPreferences(defaults:defaults),a=engine();let initial=try prefs.preset(a);defer{try? set(a,initial.options);defaults.removePersistentDomain(forName:suite)}
        try set(a,AIJSON.decode("{\"format\":\"webp\",\"webp_lossless\":false,\"quality\":83,\"width\":4,\"height\":3,\"target_space\":\"srgb\",\"allow_quantization\":true}"));try prefs.save(from:a);let saved=try XCTUnwrap(defaults.string(forKey:QuickExportPreferences.key));XCTAssertFalse(saved.contains("resource"));XCTAssertFalse(saved.contains("overwrite"));XCTAssertFalse(saved.contains("path"));let expected=try prefs.preset(a).options
        try set(a,AIJSON.decode("{\"format\":\"png\"}"));let b=engine();try prefs.restore(on:b);XCTAssertEqual(try prefs.preset(b).options,expected)
        defaults.set("{\"format\":\"jpeg\",\"path\":\"/tmp/not-authorized\"}",forKey:QuickExportPreferences.key);let before=try prefs.preset(b);XCTAssertThrowsError(try prefs.restore(on:b));XCTAssertEqual(try prefs.preset(b).version,before.version);XCTAssertEqual(try prefs.preset(b).options,before.options)
    }
    func testPresetPanelDistinguishesOriginalSizeAndRetainsCustomMatteAndExplicitQuantization()throws{
        let options=ExportOptionsController(bitDepth:8,width:16,height:12,presetEditing:true)
        try options.loadPreset(AIJSON.decode("{\"format\":\"jpeg\",\"quality\":83,\"background\":[12,34,56],\"width\":16,\"height\":12,\"allow_quantization\":true}"))
        XCTAssertTrue(options.quantization.isEnabled);XCTAssertEqual(options.background.titleOfSelectedItem,"RGB 12, 34, 56");XCTAssertEqual(try options.parameters(requireQuantization:false)["background"] as? [Int],[12,34,56]);XCTAssertEqual(try options.parameters(requireQuantization:false)["width"] as? UInt32,16);XCTAssertEqual(try options.parameters(requireQuantization:false)["allow_quantization"] as? Bool,true)
        options.width.stringValue="invalid";options.height.stringValue="0";options.originalSize.state = .on;options.changeOriginalSize();XCTAssertNil(try options.parameters(requireQuantization:false)["width"]);XCTAssertFalse(options.width.isEnabled)
        options.quantization.state = .off;XCTAssertNil(try options.parameters(requireQuantization:false)["allow_quantization"])
    }
    func testQuickExportWritesActualPngThroughSharedFfiAndPreservesSource()throws{
        let e=engine(),prefs=QuickExportPreferences();let initial=try prefs.preset(e);defer{try? set(e,initial.options)}
        _ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":8,"height":8,"bit_depth":16,"background":[10001,30003,50001,32769]]));let before=try EditorSnapshot.decode(e.snapshotJson());try set(e,AIJSON.decode("{\"format\":\"png\",\"width\":4,\"height\":3}"));let preset=try prefs.preset(e)
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent("strata-quick-\(UUID().uuidString)",isDirectory:true);try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true);defer{try? FileManager.default.removeItem(at:dir)};let path=dir.appendingPathComponent("actual.png");let resource=try e.grantWrite(path:path.path,overwrite:false)
        let result=try AIJSON.decode(e.dispatch(command:"file.quick_export",paramsJson:commandJSON(["resource":resource,"preset_version":preset.version])));XCTAssertEqual(result["width"].number,4);XCTAssertEqual(result["height"].number,3)
        let bytes=try Data(contentsOf:path);XCTAssertEqual(Array(bytes.prefix(8)),[137,80,78,71,13,10,26,10]);XCTAssertEqual(bytes[24],16)
        let after=try EditorSnapshot.decode(e.snapshotJson());XCTAssertEqual(after.revision,before.revision);XCTAssertEqual(after.historyCursor,before.historyCursor);XCTAssertEqual(after.dirty,before.dirty)
    }
    func testVisibleSheetDimensionsAndSelectAllStayInFieldWithoutEditingDocument()async throws{
        let e=engine();_ = try e.dispatch(command:"file.new",paramsJson:commandJSON(["width":1024,"height":1536,"bit_depth":8]));let controller=MainWindowController(engine:e);let window=try XCTUnwrap(controller.window),before=try e.snapshotJson()
        let options=ExportOptionsController(bitDepth:8,width:1024,height:1536,presetEditing:true);options.originalSize.state = .off;options.changeOriginalSize()
        let sheet=NSWindow(contentRect:NSRect(x:0,y:0,width:400,height:400),styleMask:[.titled],backing:.buffered,defer:false);sheet.contentView=options.view
        window.beginSheet(sheet,completionHandler:{_ in});defer{window.endSheet(sheet);sheet.orderOut(nil);controller.canvas.shutdown();window.orderOut(nil)}
        options.view.layoutSubtreeIfNeeded();XCTAssertGreaterThanOrEqual(options.width.frame.width,90);XCTAssertGreaterThanOrEqual(options.height.frame.width,90)
        options.width.selectText(nil);XCTAssertNotNil(controller.selectableText);controller.selectAllPixels(nil)
        XCTAssertEqual(controller.selectableText?.selectedRange().length,4);XCTAssertEqual(try e.snapshotJson(),before)
        let docEdit=NSMenuItem(title:"清除",action:#selector(MainWindowController.clearPixels(_:)),keyEquivalent:"");sheet.makeFirstResponder(nil);XCTAssertFalse(controller.validateMenuItem(docEdit));controller.selectAllPixels(nil);try await Task.sleep(for:.milliseconds(30));XCTAssertEqual(try e.snapshotJson(),before)
    }
}
