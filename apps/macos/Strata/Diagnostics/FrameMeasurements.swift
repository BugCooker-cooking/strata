import Foundation

struct FrameSummary: Codable {
    let frames: Int
    let elapsedSeconds: Double
    let fps: Double
    let intervalP50Ms: Double?
    let intervalP95Ms: Double?
    let maxIntervalMs: Double?
    let cpuP50Ms: Double?
    let cpuP95Ms: Double?
    let skippedFrames: Int
    var passes60HzThreshold: Bool {
        frames >= 2 && fps >= 58 && (intervalP95Ms ?? .infinity) <= 20
    }
}

enum FrameMeasurements {
    static func percentile(_ values: [Double], _ quantile: Double) -> Double? {
        guard !values.isEmpty else { return nil }
        let sorted = values.sorted()
        return sorted[max(0, min(sorted.count - 1, Int(ceil(quantile * Double(sorted.count))) - 1))]
    }

    static func summarize(timestamps: [Double], cpuMs: [Double], elapsed: Double, skipped: Int) -> FrameSummary {
        // Actual successful submission timestamps; never filter stalls or skipped-frame gaps.
        let intervals = zip(timestamps.dropFirst(), timestamps).map { ($0 - $1) * 1000 }
        return FrameSummary(frames: timestamps.count, elapsedSeconds: elapsed,
            fps: elapsed > 0 ? Double(timestamps.count) / elapsed : 0,
            intervalP50Ms: percentile(intervals, 0.5), intervalP95Ms: percentile(intervals, 0.95),
            maxIntervalMs: intervals.max(), cpuP50Ms: percentile(cpuMs, 0.5),
            cpuP95Ms: percentile(cpuMs, 0.95), skippedFrames: skipped)
    }
}
