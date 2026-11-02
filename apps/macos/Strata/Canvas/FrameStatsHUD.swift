import AppKit

@MainActor
final class FrameStatsHUD: NSTextField {
    private var timestamps: [Double] = []
    private var intervals: [Double] = []
    private var lastTimestamp: Double?

    init() {
        super.init(frame: .zero)
        isEditable = false
        isSelectable = false
        isBezeled = false
        drawsBackground = true
        backgroundColor = .windowBackgroundColor
        textColor = .secondaryLabelColor
        font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        stringValue = "M0 · GPU 预览"
        #if DEBUG
        isHidden = false
        #else
        isHidden = !ProcessInfo.processInfo.arguments.contains("--frame-stats")
        #endif
        setAccessibilityLabel("渲染帧统计")
    }

    required init?(coder: NSCoder) { nil }

    func record(timestamp: Double, cpuMs: Float) {
        guard !isHidden else { return }
        if let lastTimestamp, timestamp - lastTimestamp < 0.1 {
            intervals.append((timestamp - lastTimestamp) * 1000)
            if intervals.count > 120 { intervals.removeFirst() }
        } else { intervals.removeAll() }
        lastTimestamp = timestamp
        timestamps.append(timestamp)
        timestamps.removeAll { $0 < timestamp - 1 }
        let sorted = intervals.sorted()
        let p95 = sorted.isEmpty ? 0 : sorted[min(sorted.count - 1, Int(Double(sorted.count) * 0.95))]
        stringValue = String(format: "%d fps · CPU %.1f ms · p95 %.1f ms", timestamps.count, cpuMs, p95)
    }
}
