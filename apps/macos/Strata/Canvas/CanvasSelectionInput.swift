import AppKit

struct CanvasSelectionConfiguration: Equatable {
    var command = "select.marquee"
    var mode = "replace"
    var sample = "merged"
    var tolerance = 32
    var contiguous = true
    var radius = 8.0
    var smoothness = 8.0
}

/// A multi-click polygon or multi-stroke quick selection keeps the request
/// anchored at its first input. Worker completion cannot retarget a newer edit.
@MainActor
final class CanvasSelectionInput {
    private let engine: StrataEngine
    private var configuration = CanvasSelectionConfiguration()
    private var active = false
    private(set) var running = false
    private var mode = "replace"
    private var start = NSPoint.zero
    private var current = NSPoint.zero
    private var points: [NSPoint] = []
    private var foreground: [NSPoint] = []
    private var background: [NSPoint] = []
    private var lastBrush: NSPoint?
    private var strokeBackground = false
    var paintBackground = false
    var onChanged: (() -> Void)?
    var onRunning: ((Bool) -> Void)?
    var onCommitted: (() -> Void)?
    var onError: ((Error) -> Void)?

    init(engine: StrataEngine) { self.engine = engine }
    func configure(_ value: CanvasSelectionConfiguration) {
        if value != configuration { cancel() }
        configuration = value
    }
    func cancel() {
        if running { try? engine.cancelPendingWork() }
        else if active { try? engine.endSelection(command: configuration.command, paramsJson: "{}", cancel: true) }
        clear()
    }
    private func clear() {
        active = false; points = []; foreground = []; background = []; lastBrush = nil
        onChanged?()
    }
    private func begin(_ point: NSPoint, modifiers: NSEvent.ModifierFlags) throws {
        guard !active else { return }
        try engine.beginMarquee(); active = true; start = point; current = point
        let shift = modifiers.contains(.shift), option = modifiers.contains(.option)
        mode = shift && option ? "intersect" : shift ? "add" : option ? "subtract" : configuration.mode
    }
    func down(_ point: NSPoint, modifiers: NSEvent.ModifierFlags, clicks: Int) {
        guard !running else { return }
        do {
            try begin(point, modifiers: modifiers)
            current = point
            switch configuration.command {
            case "select.wand":
                guard let x = Int64(exactly: point.x.rounded(.down)), let y = Int64(exactly: point.y.rounded(.down)) else { cancel(); return }
                submit(["x": x, "y": y, "tolerance": configuration.tolerance, "contiguous": configuration.contiguous, "sample": configuration.sample, "mode": mode])
            case "select.quick":
                strokeBackground = paintBackground
                lastBrush = nil; brush(point)
            case "select.polygon":
                if points.count < 4096 { points.append(point) }
                if clicks > 1 { finishPath() }
            case "select.lasso": points = [point]
            default: break
            }
            onChanged?()
        } catch { cancel(); onError?(error) }
    }
    func drag(_ point: NSPoint) {
        guard active, !running else { return }
        current = point
        if configuration.command == "select.lasso" {
            if points.count >= 4096 { points = points.enumerated().filter { $0.offset.isMultiple(of: 2) }.map(\.element) }
            if let last = points.last, hypot(point.x-last.x, point.y-last.y) >= 0.5 { points.append(point) }
        } else if configuration.command == "select.quick" { brush(point) }
        onChanged?()
    }
    func hover(_ point: NSPoint) {
        guard active, configuration.command == "select.polygon" else { return }
        current = point; onChanged?()
    }
    func up(_ point: NSPoint) {
        guard active, !running else { return }
        drag(point)
        switch configuration.command {
        case "select.marquee", "select.ellipse":
            guard let x = Int64(exactly: min(start.x, current.x).rounded(.down)),
                  let y = Int64(exactly: min(start.y, current.y).rounded(.down)),
                  let width = UInt32(exactly: abs(current.x-start.x).rounded(.up)),
                  let height = UInt32(exactly: abs(current.y-start.y).rounded(.up)), width > 0, height > 0 else { cancel(); return }
            submit(["x": x, "y": y, "width": width, "height": height, "mode": mode])
        case "select.lasso": finishPath()
        case "select.quick": lastBrush = nil
        default: break
        }
    }
    func confirm() -> Bool {
        guard active, !running else { return false }
        if configuration.command == "select.polygon" { finishPath(); return true }
        if configuration.command == "select.quick" {
            guard !foreground.isEmpty else { return true }
            submit(["foreground": coordinates(foreground), "background": coordinates(background), "radius": configuration.radius,
                    "smoothness": configuration.smoothness, "sample": configuration.sample, "mode": mode])
            return true
        }
        return false
    }
    private func coordinates(_ points: [NSPoint]) -> [[Double]] { points.map { [Double($0.x), Double($0.y)] } }
    private func finishPath() {
        guard points.count >= 3 else { cancel(); return }
        submit(["points": coordinates(points), "mode": mode])
    }
    private func brush(_ point: NSPoint) {
        let last = lastBrush ?? point
        let length = hypot(point.x-last.x, point.y-last.y)
        let available = max(0, 4096-foreground.count-background.count)
        let steps = min(available, max(1, Int(min(4096, ceil(length/max(0.5, configuration.radius/2))))))
        guard steps > 0 else { return }
        for i in 1...steps {
            let t = CGFloat(i)/CGFloat(steps)
            let seed = NSPoint(x: last.x+(point.x-last.x)*t, y: last.y+(point.y-last.y)*t)
            if strokeBackground { background.append(seed) } else { foreground.append(seed) }
        }
        lastBrush = point
    }
    private func submit(_ params: [String: Any]) {
        do {
            let encoded = try commandJSON(params), command = configuration.command, engine = engine
            active = false; running = true; onRunning?(true)
            Task { [weak self] in
                let result: Result<Void, Error>
                do { try await aiEngineWork { try engine.endSelection(command: command, paramsJson: encoded, cancel: false) }; result = .success(()) }
                catch { result = .failure(error) }
                guard let self else { return }
                self.running = false; self.clear(); self.onRunning?(false)
                switch result { case .success: self.onCommitted?(); case .failure(let error): self.onError?(error) }
            }
        } catch { cancel(); onError?(error) }
    }
    func paths() -> (CGPath?, CGPath?, CGPath?) {
        guard active || running else { return (nil, nil, nil) }
        let path = CGMutablePath()
        switch configuration.command {
        case "select.marquee", "select.ellipse":
            let rect = CGRect(x: min(start.x,current.x), y: min(start.y,current.y), width: abs(current.x-start.x), height: abs(current.y-start.y))
            if configuration.command == "select.ellipse" { path.addEllipse(in: rect) } else { path.addRect(rect) }
        case "select.polygon", "select.lasso":
            if let first = points.first { path.move(to: first); for point in points.dropFirst() { path.addLine(to: point) }; path.addLine(to: current); path.closeSubpath() }
        default: break
        }
        func brushes(_ points: [NSPoint]) -> CGPath {
            let out = CGMutablePath(), radius = configuration.radius
            for p in points { out.addEllipse(in: CGRect(x: p.x-radius, y: p.y-radius, width: radius*2, height: radius*2)) }
            return out
        }
        return (path, brushes(foreground), brushes(background))
    }
}
