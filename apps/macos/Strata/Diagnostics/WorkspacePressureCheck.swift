import AppKit

/// Opt-in native, multi-engine resource check. It uses the same windows,
/// opening, thumbnail and CAMetalLayer paths as ordinary document tabs.
@MainActor
enum WorkspacePressureCheck {
    struct Options {
        let inputs: [URL]
        let output: URL
        static func parse(_ args: [String]) throws -> Options? {
            guard args.contains("--workspace-pressure-input") else { return nil }
            func values(_ flag: String) throws -> [String] {
                try args.indices.filter { args[$0] == flag }.map {
                    guard $0 + 1 < args.count, !args[$0 + 1].hasPrefix("--") else {
                        throw BenchmarkFailure("Missing value for \(flag)")
                    }
                    return args[$0 + 1]
                }
            }
            let inputs = try values("--workspace-pressure-input").map { URL(fileURLWithPath: $0) }
            let outputs = try values("--workspace-pressure-output")
            guard inputs.count >= 2, outputs.count == 1 else {
                throw BenchmarkFailure("Workspace pressure check requires at least two inputs and one output")
            }
            let output = URL(fileURLWithPath: outputs[0])
            guard !FileManager.default.fileExists(atPath: output.path) else {
                throw BenchmarkFailure("Refusing to overwrite pressure report")
            }
            return Options(inputs: inputs, output: output)
        }
    }

    static func run(_ options: Options, controllers: [MainWindowController]) async throws {
        var states: [String] = []
        var previews: [[Data]] = []
        var documents: [Any] = []
        for (index, controller) in controllers.enumerated() {
            controller.window?.level = .floating
            controller.window?.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            controller.showWindow(nil); controller.window?.makeKeyAndOrderFront(nil)
            benchmarkLog("STRATA_WORKSPACE_PRESSURE open=\(index + 1)/\(controllers.count)")
            let info = try await withCheckedThrowingContinuation { continuation in
                controller.open(options.inputs[index], recordRecent: false) { continuation.resume(with: $0) }
            }
            guard info.width == 6000, info.height == 4000 else {
                throw BenchmarkFailure("Pressure fixtures must be 24MP")
            }
            let state = try controller.engine.snapshotJson()
            let revision = try EditorSnapshot.decode(state).revision
            let engine = controller.engine
            let thumbnails = try await Task.detached { try engine.layerThumbnails(revision: revision).map(\.rgba) }.value
            guard thumbnails.count == 20 else { throw BenchmarkFailure("Expected 20 thumbnails per document") }
            states.append(state); previews.append(thumbnails)
            documents.append(try object(engine.documentMemoryJson()))
        }
        let engine = controllers[0].engine
        let before = try object(engine.memoryStatsJson())
        // Traverse the whole source at 1:1, accumulating native GPU caches
        // across engines rather than duplicating one layer or one renderer.
        for controller in controllers {
            controller.window?.makeKeyAndOrderFront(nil)
            try await Task.sleep(for: .milliseconds(100))
            let scale = Float(controller.window?.backingScaleFactor ?? 1)
            try controller.engine.zoomAt(x: Float(controller.canvas.bounds.midX) * scale,
                                         y: Float(controller.canvas.bounds.midY) * scale,
                                         factor: 1 / controller.engine.canvasZoom())
            var previousX: Float = 0, previousY: Float = 0
            for y in [-1500, 0, 1500] {
                for x in [-2400, -1200, 0, 1200, 2400] {
                    try controller.engine.panBy(dx: Float(x) - previousX, dy: Float(y) - previousY)
                    previousX = Float(x); previousY = Float(y)
                    let frame = try controller.engine.renderFrame()
                    guard !frame.skipped else { throw BenchmarkFailure("Native pressure frame skipped") }
                    await Task.yield()
                }
            }
            benchmarkLog("STRATA_WORKSPACE_PRESSURE traversed native document")
        }
        let populated = try object(engine.memoryStatsJson())
        try await Task.detached { try engine.trimMemory() }.value
        let trimmed = try object(engine.memoryStatsJson())
        for (index, controller) in controllers.enumerated() {
            controller.window?.makeKeyAndOrderFront(nil)
            try await Task.sleep(for: .milliseconds(100))
            try controller.engine.fitToScreen()
            guard !(try controller.engine.renderFrame()).skipped else { throw BenchmarkFailure("Revisit frame skipped") }
            let current = try controller.engine.snapshotJson()
            guard current == states[index] else { throw BenchmarkFailure("Document state changed under pressure") }
            let revision = try EditorSnapshot.decode(current).revision, currentEngine = controller.engine
            let thumbnails = try await Task.detached { try currentEngine.layerThumbnails(revision: revision).map(\.rgba) }.value
            guard thumbnails == previews[index] else { throw BenchmarkFailure("Thumbnail samples changed under pressure") }
        }
        let revisited = try object(engine.memoryStatsJson())
        for controller in controllers { controller.canvas.shutdown() }
        let detached = try object(engine.memoryStatsJson())
        guard let stats = revisited as? [String: Any], let usage = stats["usage"] as? [String: Any],
              let limits = stats["limits"] as? [String: Any],
              let gpuEvictions = usage["gpu_evictions"] as? NSNumber, gpuEvictions.uint64Value > 0,
              let peakCPU = usage["peak_cpu_bytes"] as? NSNumber, let cpuLimit = limits["cpu_bytes"] as? NSNumber,
              let peakGPU = usage["peak_gpu_bytes"] as? NSNumber, let gpuLimit = limits["gpu_bytes"] as? NSNumber,
              peakCPU.uint64Value <= cpuLimit.uint64Value, peakGPU.uint64Value <= gpuLimit.uint64Value,
              let detachedStats = detached as? [String: Any], let detachedUsage = detachedStats["usage"] as? [String: Any],
              (detachedUsage["gpu_bytes"] as? NSNumber)?.uint64Value == 0 else {
            throw BenchmarkFailure("Pressure check did not prove bounded GPU eviction and complete canvas release")
        }
        let report: [String: Any] = [
            "date": ISO8601DateFormatter().string(from: Date()),
            "source_revision": ProcessInfo.processInfo.environment["STRATA_SOURCE_REVISION"] ?? "unrecorded",
            "passed": true, "documents": documents, "native_renderers": controllers.count,
            "explicit_native_frames_per_document": 16, "thumbnail_sample_bytes_exact": previews.map { $0.reduce(0) { $0 + $1.count } },
            "state_exact": true, "before": before, "populated": populated, "trimmed": trimmed,
            "revisited": revisited, "after_canvas_detach": detached,
            "limitations": ["Native resource pressure check, not a frame-rate measurement.",
                            "Thumbnail samples compared exactly; full source sample preservation is covered separately by the 50MP stress check."]
        ]
        try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            .write(to: options.output, options: .withoutOverwriting)
        benchmarkLog("STRATA_WORKSPACE_PRESSURE_REPORT \(options.output.path)")
    }

    private static func object(_ json: String) throws -> Any {
        try JSONSerialization.jsonObject(with: Data(json.utf8))
    }
}
