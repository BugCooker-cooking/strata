import XCTest
@testable import Strata

final class BenchmarkTests: XCTestCase {
    func testCallbackRateUsesDisplayDivisorAtOrAboveSixtyWhenAvailable() {
        for (maximum, expected) in [(60, 60), (120, 60), (144, 72), (160, 80), (240, 60), (30, 30)] {
            XCTAssertEqual(FrameMeasurements.callbackRate(maximumFPS: maximum), expected)
        }
    }

    func testSixtyHzSequencePassesHostThreshold() {
        let summary = FrameMeasurements.summarize(timestamps: (0..<1800).map { Double($0) / 60 },
            cpuMs: Array(repeating: 1.0, count: 1800), elapsed: 30, skipped: 0)
        XCTAssertEqual(summary.fps, 60, accuracy: 0.001)
        XCTAssertEqual(summary.intervalP95Ms ?? 0, 1000 / 60, accuracy: 0.001)
        XCTAssertTrue(summary.passes60HzThreshold)
    }

    func testLongStallsAreNotDiscarded() {
        let summary = FrameMeasurements.summarize(timestamps: [0, 0.016, 0.032, 0.232, 0.248],
            cpuMs: [1, 1, 1, 1, 1], elapsed: 0.3, skipped: 2)
        XCTAssertEqual(summary.intervalP95Ms ?? 0, 200, accuracy: 0.001)
        XCTAssertEqual(summary.maxIntervalMs ?? 0, 200, accuracy: 0.001)
        XCTAssertEqual(summary.skippedFrames, 2)
        XCTAssertFalse(summary.passes60HzThreshold)
    }

    func testEmptyAndSingleSamplesCannotPass() {
        XCTAssertFalse(FrameMeasurements.summarize(timestamps: [], cpuMs: [], elapsed: 30, skipped: 0).passes60HzThreshold)
        XCTAssertFalse(FrameMeasurements.summarize(timestamps: [0], cpuMs: [1], elapsed: 0.001, skipped: 0).passes60HzThreshold)
    }
}
