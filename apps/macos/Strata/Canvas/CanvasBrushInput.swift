import AppKit

struct CanvasBrushConfiguration: Equatable {
    var layer: String?
    var erase = false
    var target = "pixels"
    var size = 24.0, hardness = 0.8, opacity = 1.0, flow = 1.0, spacing = 0.2
    var color = [0.0,0.0,0.0,1.0], value = 1.0
    var pressureSize = true, pressureOpacity = true
    var clone = false, aligned = true, cloneSource: [Double]?, cloneSample = "current_layer"
    var heal = false
    var spot = false
    var spotHealMode = "proximity_match"
    var spotSearchRadius = 64.0
    var tonalCommand:String?
    var tonalRange = "midtones", tonalExposure = 0.5
    var protectTones = true, spongeMode = "desaturate", spongeVibrance = true
}
@MainActor
final class CanvasBrushInput {
    private let engine: StrataEngine
    private(set) var configuration = CanvasBrushConfiguration()
    private var points: [(point: NSPoint, pressure: Double)] = []
    private var active = false
    private var cloneDestination:NSPoint?
    private var activeDestination:NSPoint?
    private(set) var running = false
    var onChanged: (() -> Void)?
    var onRunning: ((Bool) -> Void)?
    var onCommitted: (() -> Void)?
    var onError: ((Error) -> Void)?
    init(engine: StrataEngine) { self.engine = engine }
    func configure(_ value: CanvasBrushConfiguration) {
        if configuration != value { cancel() }
        if configuration.layer != value.layer || configuration.cloneSource != value.cloneSource || configuration.aligned != value.aligned || configuration.spot != value.spot || configuration.heal != value.heal {cloneDestination=nil}
        configuration = value
    }
    func down(_ point: NSPoint, pressure: Double) {
        guard !running, let layer = configuration.layer else { return }
        do {
            if let command=configuration.tonalCommand{
                try engine.beginTonalBrush(layerId:layer,command:command)
            }else if configuration.spot {
                try engine.beginSpotHeal(layerId:layer)
            }else if configuration.clone {
                guard configuration.cloneSource != nil else{throw AIClientError.message(L10n.text("请先按住 ⌥ 单击画布设置取样源点。"))}
                if configuration.heal{try engine.beginHeal(layerId:layer)}else{try engine.beginClone(layerId:layer)}
                activeDestination=configuration.aligned ? cloneDestination ?? point : point
            }else{try engine.beginBrush(layerId: layer, target: configuration.target, erase: configuration.erase)}
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
            var parameters: [String:Any] = ["points":path,"size":settings.size,"hardness":settings.hardness,
                "opacity":settings.opacity,"flow":settings.flow,"spacing":settings.spacing,"color":settings.color,
                "value":settings.value,"pressure_size":settings.pressureSize,"pressure_opacity":settings.pressureOpacity]
            let destination=activeDestination
            if settings.clone,let source=settings.cloneSource,let destination {parameters["clone_source"]=["source":source,"destination":[Double(destination.x),Double(destination.y)],"sample":settings.cloneSample]}
            var payload:[String:Any] = settings.heal || settings.spot || settings.tonalCommand != nil ? ["stroke":parameters] : parameters
            if settings.spot {
                guard settings.spotSearchRadius.isFinite,(1...512).contains(settings.spotSearchRadius) else{throw AIClientError.message(L10n.text("污点修复搜索范围需要在 1–512px 内。"))}
                payload["sample"]=settings.cloneSample;payload["search_radius"]=Int(settings.spotSearchRadius)
                payload["mode"] = settings.spotHealMode
            }
            if let command=settings.tonalCommand {
                if command == "retouch.sponge" {payload["amount"]=settings.tonalExposure;payload["mode"]=settings.spongeMode;payload["vibrance"]=settings.spongeVibrance}
                else {payload["exposure"]=settings.tonalExposure;payload["range"]=settings.tonalRange;payload["protect_tones"]=settings.protectTones}
            }
            let encoded = try commandJSON(payload), engine = engine
            active = false; running = true; onRunning?(true)
            Task { [weak self] in
                let result: Result<Void,Error>
                do { try await aiEngineWork { try engine.endBrush(paramsJson: encoded, cancel: false) }; result = .success(()) }
                catch { result = .failure(error) }
                guard let self else { return }
                self.running = false; self.points = []; self.onChanged?(); self.onRunning?(false)
                switch result { case .success:if settings.clone,settings.aligned{self.cloneDestination=destination};self.onCommitted?(); case .failure(let error):self.onError?(error) }
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
