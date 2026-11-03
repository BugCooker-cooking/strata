import AppKit
import Darwin
import QuartzCore

struct BenchmarkOptions {
    let input: URL
    let output: URL
    let duration: Double
    let runs: Int
    let keepVisible: Bool

    static func parse(_ args: [String]) throws -> BenchmarkOptions? {
        guard args.contains("--benchmark-input") else { return nil }
        func value(_ name: String) throws -> String {
            guard let i = args.firstIndex(of: name), i + 1 < args.count, !args[i+1].hasPrefix("--") else {
                throw BenchmarkFailure("Missing value for \(name)")
            }
            return args[i+1]
        }
        let input = URL(fileURLWithPath: try value("--benchmark-input"))
        let output = URL(fileURLWithPath: try value("--benchmark-output"))
        let seconds = args.contains("--benchmark-seconds") ? Double(try value("--benchmark-seconds")) : 30
        let runs = args.contains("--benchmark-runs") ? Int(try value("--benchmark-runs")) : 3
        guard let seconds, seconds.isFinite, (1...120).contains(seconds), let runs, (1...10).contains(runs) else {
            throw BenchmarkFailure("Benchmark duration must be 1…120 seconds and runs 1…10")
        }
        guard !FileManager.default.fileExists(atPath: output.path) else {
            throw BenchmarkFailure("Refusing to overwrite benchmark output: \(output.path)")
        }
        return BenchmarkOptions(input: input, output: output, duration: seconds, runs: runs,
                                keepVisible: args.contains("--benchmark-keep-visible"))
    }
}

struct BenchmarkFailure: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

private struct BenchmarkRun: Codable {
    let number: Int
    let summary: FrameSummary
    let timestampsSeconds: [Double]
    let cpuMs: [Double]
}

private struct BenchmarkReport: Encodable {
    let schemaVersion = 1
    let configuration: String
    let date: String
    let sourceRevision: String
    let imageWidth: UInt32
    let imageHeight: UInt32
    let imageBitDepth: UInt8
    let viewportWidth: UInt32
    let viewportHeight: UInt32
    let backingScale: Float
    let screenName: String
    let screenColorSpace: String
    let screenMaximumFPS: Int
    let requestedCallbackFPS: Int
    let floatingWindow: Bool
    let displayLinkNominalHz: Double
    let osVersion: String
    let processor: String
    let memoryBytes: UInt64
    let openMilliseconds: Double
    let warmupSecondsPerRun: Double
    let processPeakRSSBytes: UInt64?
    let maximumObservedRenderingFootprintBytes: UInt64?
    let runs: [BenchmarkRun]
    let passesHost60HzThreshold: Bool
    let qualifiesAsM0HostRun: Bool
    let limitations: [String]
}

@MainActor
final class CanvasBenchmark {
    private let options: BenchmarkOptions
    private let engine: StrataEngine
    private let info: ImageInfo
    private let initialViewport: CanvasViewport
    private let screen: NSScreen
    private let openMs: Double
    private let completion: (Result<URL, Error>) -> Void
    private var trialStarted: Double?
    private var measurementStarted: Double?
    private var timestamps: [Double] = []
    private var cpu: [Double] = []
    private var skipped = 0
    private var trials: [BenchmarkRun] = []
    private var nominalHz: Double = 0
    private var maximumFootprint: UInt64?
    private var previousX: Double = 0
    private var previousY: Double = 0
    private var previousZoom: Double = 1
    private(set) var finished = false
    var hasStarted: Bool { trialStarted != nil || !trials.isEmpty }
    private let warmup: Double = 2

    init(options: BenchmarkOptions, engine: StrataEngine, info: ImageInfo, viewport: CanvasViewport,
         screen: NSScreen, openMs: Double, completion: @escaping (Result<URL, Error>) -> Void) {
        self.options = options; self.engine = engine; self.info = info
        initialViewport = viewport; self.screen = screen; self.openMs = openMs
        self.completion = completion
    }

    func beforeFrame(viewport: CanvasViewport, window: NSWindow?, link: CADisplayLink) throws {
        guard !finished else { return }
        let sameScreen = window?.screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber
            == screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber
        guard viewport == initialViewport, sameScreen,
              window?.isMiniaturized == false, window?.occlusionState.contains(.visible) == true else {
            throw BenchmarkFailure("Benchmark interrupted: viewport=\(viewport.width)x\(viewport.height) scale=\(viewport.scale), expected=\(initialViewport.width)x\(initialViewport.height) scale=\(initialViewport.scale), sameDisplay=\(sameScreen), minimized=\(window?.isMiniaturized ?? true), visible=\(window?.occlusionState.contains(.visible) ?? false)")
        }
        let now = CACurrentMediaTime()
        if trialStarted == nil {
            try engine.fitToScreen()
            trialStarted = now; previousX = 0; previousY = 0; previousZoom = 1
            print("STRATA_BENCHMARK run=\(trials.count+1)/\(options.runs) warmup=2s measured=\(options.duration)s")
        }
        if nominalHz == 0, link.duration > 0 { nominalHz = 1 / link.duration }
        let t = now - (trialStarted ?? now)
        // Internal, fixed physical-pixel command trajectory, not synthetic mouse/trackpad events.
        let x = 240 * sin(t * 0.7), y = 160 * sin(t * 0.43)
        let zoom = 1 + 0.35 * pow(sin(t * 0.31), 2)
        try engine.zoomAt(x: Float(viewport.width)/2, y: Float(viewport.height)/2,
                          factor: Float(zoom / previousZoom))
        try engine.panBy(dx: Float(x - previousX), dy: Float(y - previousY))
        previousX = x; previousY = y; previousZoom = zoom
        if t >= warmup, measurementStarted == nil { measurementStarted = now }
    }

    func afterFrame(_ frame: FrameInfo) {
        guard !finished else { return }
        let now = CACurrentMediaTime()
        if let bytes = Self.physicalFootprint() { maximumFootprint = max(maximumFootprint ?? 0, bytes) }
        guard let start = measurementStarted else { return }
        if frame.skipped { skipped += 1 }
        else { timestamps.append(now - start); cpu.append(Double(frame.cpuMs)) }
        let elapsed = now - start
        if elapsed >= options.duration {
            let summary = FrameMeasurements.summarize(timestamps: timestamps, cpuMs: cpu, elapsed: elapsed, skipped: skipped)
            trials.append(BenchmarkRun(number: trials.count+1, summary: summary, timestampsSeconds: timestamps, cpuMs: cpu))
            print(String(format: "STRATA_BENCHMARK fps=%.2f p50=%.2fms p95=%.2fms max=%.2fms skipped=%d",
                         summary.fps, summary.intervalP50Ms ?? 0, summary.intervalP95Ms ?? 0,
                         summary.maxIntervalMs ?? 0, skipped))
            trialStarted = nil; measurementStarted = nil; timestamps = []; cpu = []; skipped = 0
            if trials.count == options.runs { finish() }
        }
    }

    func cancel(_ error: Error) {
        guard !finished else { return }
        finished = true
        completion(.failure(error))
    }

    private func finish() {
        finished = true
        #if DEBUG
        let configuration = "Debug"
        #else
        let configuration = "Release"
        #endif
        let thresholds = trials.allSatisfy { $0.summary.passes60HzThreshold }
        let report = BenchmarkReport(configuration: configuration, date: ISO8601DateFormatter().string(from: Date()),
            sourceRevision: ProcessInfo.processInfo.environment["STRATA_SOURCE_REVISION"] ?? "unrecorded",
            imageWidth: info.width, imageHeight: info.height, imageBitDepth: info.bitDepth,
            viewportWidth: initialViewport.width, viewportHeight: initialViewport.height, backingScale: initialViewport.scale,
            screenName: screen.localizedName, screenColorSpace: screen.colorSpace?.localizedName ?? "unknown",
            screenMaximumFPS: screen.maximumFramesPerSecond,
            requestedCallbackFPS: FrameMeasurements.callbackRate(maximumFPS: screen.maximumFramesPerSecond),
            floatingWindow: options.keepVisible, displayLinkNominalHz: nominalHz,
            osVersion: ProcessInfo.processInfo.operatingSystemVersionString, processor: Self.processor(),
            memoryBytes: ProcessInfo.processInfo.physicalMemory, openMilliseconds: openMs, warmupSecondsPerRun: warmup,
            processPeakRSSBytes: Self.peakRSS(), maximumObservedRenderingFootprintBytes: maximumFootprint, runs: trials,
            passesHost60HzThreshold: thresholds,
            qualifiesAsM0HostRun: thresholds && configuration == "Release" && options.duration >= 30 && options.runs >= 3
                && UInt64(info.width) * UInt64(info.height) == 24_000_000 && nominalHz >= 59,
            limitations: ["Actual successful CPU submission intervals, not GPU completion or presentation timestamps.",
                "Deterministic command trajectory; does not validate physical trackpad latency or cross-display gestures.",
                "Fresh application open; OS file-cache coldness is uncontrolled. First trial vs later trials reported separately.",
                "RSS high-water includes opening; physical footprint sampled after render, not an exact peak during decode.",
                "Host result only; does not establish the M1 MacBook Air baseline. No pixels or credentials in report."])
        do {
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(report).write(to: options.output, options: .withoutOverwriting)
            completion(.success(options.output))
        } catch { completion(.failure(error)) }
    }

    private static func peakRSS() -> UInt64? {
        var usage = rusage()
        return getrusage(RUSAGE_SELF, &usage) == 0 ? UInt64(usage.ru_maxrss) : nil
    }

    private static func physicalFootprint() -> UInt64? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let result = withUnsafeMutablePointer(to: &info) { ptr in
            ptr.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? info.phys_footprint : nil
    }

    private static func processor() -> String {
        var size = 0
        guard sysctlbyname("machdep.cpu.brand_string", nil, &size, nil, 0) == 0, size > 0 else { return "unknown" }
        var bytes = [CChar](repeating: 0, count: size)
        guard sysctlbyname("machdep.cpu.brand_string", &bytes, &size, nil, 0) == 0 else { return "unknown" }
        return String(decoding: bytes.prefix(while: { $0 != 0 }).map { UInt8(bitPattern: $0) }, as: UTF8.self)
    }
}
