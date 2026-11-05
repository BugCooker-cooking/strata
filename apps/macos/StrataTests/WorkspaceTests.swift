import AppKit
import XCTest
@testable import Strata

final class WorkspaceTests: XCTestCase {
    @MainActor
    func testBusyNavigationAndExplicitCancellationKeepTheWorkspaceResponsive() throws {
        let engine=StrataEngine(listener:EngineEvents{_ in})
        let controller=MainWindowController(engine:engine)
        defer{controller.canvas.shutdown();controller.window?.orderOut(nil)}
        controller.busy=true
        XCTAssertTrue(controller.canvas.isWorkBusy)
        let tool=controller.workspace.tool
        controller.workspace.selectTool(.zoom)
        XCTAssertEqual(controller.workspace.tool,tool)
        controller.canvas.fit();controller.canvas.zoomBy(2);controller.canvas.actualSize()
        XCTAssertNil(controller.window?.attachedSheet)
        controller.showError(StrataError.Cancelled(message:"user cancelled"))
        XCTAssertNil(controller.window?.attachedSheet,"An explicit cancellation should not open a modal error")
        controller.busy=false
        controller.workspace.selectTool(.zoom)
        XCTAssertEqual(controller.workspace.tool,.zoom)
    }
    @MainActor
    func testMemoryPressureCallbackRunsOnUtilityQueueAndPreservesDocument() async throws {
        let handler = AppDelegate.makeMemoryPressureHandler()
        weak var retiredEngine: StrataEngine?
        do {
            let retired = StrataEngine(listener: EngineEvents { _ in })
            _ = try retired.dispatch(command: "file.new", paramsJson: commandJSON(["width": 16, "height": 16, "bit_depth": 16]))
            retiredEngine = retired
        }
        XCTAssertNil(retiredEngine, "The global pressure callback must not retain a closed document")
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 2, "height": 2, "bit_depth": 16]))
        _ = try engine.dispatch(command: "layer.create", paramsJson: commandJSON(["name": "pressure", "fill": [32768,32769,50001,12345]]))
        let state = try engine.snapshotJson()
        let revision = try EditorSnapshot.decode(state).revision
        let previews = try engine.layerThumbnails(revision: revision).map(\.rgba)
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                handler()
                continuation.resume()
            }
        }
        XCTAssertEqual(try engine.snapshotJson(), state)
        XCTAssertEqual(try engine.layerThumbnails(revision: revision).map(\.rgba), previews)
    }
    @MainActor
    func testMinimumWorkspaceKeepsAllThreePanelsInsideContentBounds() throws {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.dispatch(command: "file.new", paramsJson: commandJSON(["width": 16, "height": 16, "bit_depth": 8]))
        let controller = MainWindowController(engine: engine)
        let window = try XCTUnwrap(controller.window)
        defer { controller.canvas.shutdown() }
        window.setFrame(NSRect(origin: .zero, size: window.minSize), display: false)
        let content = try XCTUnwrap(window.contentView)
        content.layoutSubtreeIfNeeded()
        func findSplit(_ view: NSView) -> NSSplitView? {
            if let split = view as? NSSplitView { return split }
            return view.subviews.lazy.compactMap(findSplit).first
        }
        let split = try XCTUnwrap(findSplit(content))
        XCTAssertEqual(split.arrangedSubviews.count, 3)
        XCTAssertGreaterThanOrEqual(split.arrangedSubviews[0].frame.width, 400)
        XCTAssertGreaterThanOrEqual(split.arrangedSubviews[1].frame.width, 250)
        XCTAssertGreaterThanOrEqual(split.arrangedSubviews[2].frame.width, 300)
        for panel in split.arrangedSubviews {
            XCTAssertLessThanOrEqual(panel.frame.maxX, split.bounds.maxX + 1)
            XCTAssertGreaterThan(panel.frame.height, 500)
        }
        controller.toggleAssistant()
        content.layoutSubtreeIfNeeded()
        XCTAssertFalse(controller.workspace.assistantVisible)
        controller.toggleAssistant()
        content.layoutSubtreeIfNeeded()
        XCTAssertTrue(controller.workspace.assistantVisible)
    }

    func testRecentFilesAreNormalizedDeduplicatedCappedAndPersistent() throws {
        let suite = "Strata.WorkspaceTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = RecentFileStore(defaults: defaults)
        for index in 0..<12 { store.record(URL(fileURLWithPath: "/tmp/strata-recent-\(index).png")) }
        store.record(URL(fileURLWithPath: "/tmp/other/../strata-recent-5.png"))
        XCTAssertEqual(store.urls.count, 10)
        XCTAssertEqual(store.urls.first?.path, "/tmp/strata-recent-5.png")
        XCTAssertEqual(Set(store.urls).count, 10)
        XCTAssertEqual(RecentFileStore(defaults: defaults).urls, store.urls)
        store.clear()
        XCTAssertTrue(store.urls.isEmpty)
    }

    func testControlPolicyPreservesViewAccessAndFailsClosed() {
        let running = WorkspacePermissions(busy: false, control: "running")
        XCTAssertTrue(running.selection)
        XCTAssertFalse(running.edits || running.history || running.visibility)
        let paused = WorkspacePermissions(busy: false, control: "paused")
        XCTAssertTrue(paused.history && paused.visibility && paused.selection)
        XCTAssertFalse(paused.edits)
        for control in ["user", "paused", "running"] {
            let busy = WorkspacePermissions(busy: true, control: control)
            XCTAssertFalse(busy.edits || busy.history || busy.visibility || busy.selection)
        }
        let unknown = WorkspacePermissions(busy: false, control: "unknown")
        XCTAssertFalse(unknown.edits || unknown.history || unknown.visibility || unknown.selection)
    }

    func testDocumentEnginesKeepSelectionHistoryAndDepthIndependent() throws {
        let first = StrataEngine(listener: EngineEvents { _ in })
        let second = StrataEngine(listener: EngineEvents { _ in })
        _ = try first.dispatch(command: "file.new", paramsJson: commandJSON(["width": 7, "height": 9, "bit_depth": 8]))
        _ = try second.dispatch(command: "file.new", paramsJson: commandJSON(["width": 11, "height": 13, "bit_depth": 16]))
        let untouched = try second.snapshotJson()
        _ = try first.dispatch(command: "layer.create", paramsJson: commandJSON(["name": "Only first tab"]))
        XCTAssertEqual(try second.snapshotJson(), untouched)
        XCTAssertEqual(try EditorSnapshot.decode(first.snapshotJson()).layers.count, 2)
        _ = try first.dispatch(command: "edit.undo", paramsJson: "{}")
        XCTAssertEqual(try second.snapshotJson(), untouched)
        XCTAssertEqual(try EditorSnapshot.decode(first.snapshotJson()).layers.count, 1)
        XCTAssertEqual(try EditorSnapshot.decode(second.snapshotJson()).bitDepth, 16)
    }
}
