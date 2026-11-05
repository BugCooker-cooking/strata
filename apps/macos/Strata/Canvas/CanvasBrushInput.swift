import AppKit

struct CanvasBrushConfiguration: Equatable {
    var layer: String?
    var erase = false
    var target = "pixels"
    var size = 24.0, hardness = 0.8, opacity = 1.0, flow = 1.0, spacing = 0.2
    var color = [0.0,0.0,0.0,1.0], value = 1.0
    var pressureSize = true, pressureOpacity = true
}
@MainActor
final class CanvasBrushInput {
    private let engine: StrataEngine
    private(set) var configuration = CanvasBrushConfiguration()
    private var points: [(point: NSPoint, pressure: Double)] = []
    private var active = false
    private(set) var running = false
    var onChanged: (() -> Void)?
    var onRunning: ((Bool) -> Void)?
    var onCommitted: (() -> Void)?
    var onError: ((Error) -> Void)?
    init(engine: StrataEngine) { self.engine = engine }
    func configure(_ value: CanvasBrushConfiguration) {
        if configuration != value { cancel() }
        configuration = value
    }
    func down(_ point: NSPoint, pressure: Double) {
        guard !running, let layer = configuration.layer else { return }
        do {
            try engine.beginBrush(layerId: layer, target: configuration.target, erase: configuration.erase)
            active = true; points = [(point,pressure)]; onChanged?()
        } catch { onError?(error) }
    }
    func drag(_ point: NSPoint, pressure: Double) {
        guard active, !running else { return }
        if points.count >= 8192 { points = points.enumerated().filter { $0.offset.isMultiple(of: 2) }.map(\.element) }
        points.append((point,pressure)); onChanged?()
    }
    func up(_ point: NSPoint, pressure: Double) {
        guard active, !running else { return }
        drag(point, pressure: pressure)
        do {
            let settings = configuration
            let path = points.map { ["x":Double($0.point.x),"y":Double($0.point.y),"pressure":$0.pressure] }
            let parameters: [String:Any] = ["points":path,"size":settings.size,"hardness":settings.hardness,
                "opacity":settings.opacity,"flow":settings.flow,"spacing":settings.spacing,"color":settings.color,
                "value":settings.value,"pressure_size":settings.pressureSize,"pressure_opacity":settings.pressureOpacity]
            let encoded = try commandJSON(parameters), engine = engine
            active = false; running = true; onRunning?(true)
            Task { [weak self] in
                let result: Result<Void,Error>
                do { try await aiEngineWork { try engine.endBrush(paramsJson: encoded, cancel: false) }; result = .success(()) }
                catch { result = .failure(error) }
                guard let self else { return }
                self.running = false; self.points = []; self.onChanged?(); self.onRunning?(false)
                switch result { case .success:self.onCommitted?(); case .failure(let error):self.onError?(error) }
            }
        } catch { cancel(); onError?(error) }
    }
    func cancel() {
        if running { try? engine.cancelPendingWork() }
        else if active { try? engine.endBrush(paramsJson: "{}", cancel: true) }
        let changed = active || !points.isEmpty
        active = false; points = []
        if changed { onChanged?() }
    }
    /// Temporary trajectory feedback while input is active. Actual pressure,
    /// soft-tip coverage and linear compositing are evaluated by the core.
    func path() -> CGPath? {
        guard let first = points.first, active || running else { return nil }
        let path = CGMutablePath();path.move(to:first.point)
        for point in points.dropFirst() { path.addLine(to:point.point) }
        if points.count == 1 { path.addLine(to:NSPoint(x:first.point.x+0.01,y:first.point.y)) }
        return path
    }
}
