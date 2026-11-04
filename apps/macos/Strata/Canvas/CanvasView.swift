import AppKit
import QuartzCore

@MainActor
final class CanvasView: NSView {
    private let engine: StrataEngine
    private var frameLink: CADisplayLink?
    private var attached = false
    private var dirty = true
    private var lastDrag: NSPoint?
    private let stats = FrameStatsHUD()
    private let artboardBorder = CAShapeLayer()
    private let artboardTitle = CATextLayer()
    private var benchmark: CanvasBenchmark?
    var onError: ((Error) -> Void)?
    var onCommitted: (() -> Void)?
    var onViewChanged: ((ArtboardOverlay?, Double, CGFloat) -> Void)?
    var onPointerMoved: ((NSPoint) -> Void)?
    var onToolKey: ((String) -> Bool)?
    var zoomTool = false
    private var pointerTracking: NSTrackingArea?
    var movesLayer = false
    var isEditingEnabled = true
    var activeLayer: EditorSnapshot.Layer?
    private var layerDrag: (point: NSPoint, x: Int64, y: Int64, zoom: CGFloat)?
    private var layerPosition: (x: Int64, y: Int64)?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    init(engine: StrataEngine) {
        self.engine = engine
        super.init(frame: .zero)
        wantsLayer = true
        layer = makeBackingLayer()
        artboardBorder.fillColor = nil
        artboardBorder.strokeColor = NSColor.white.withAlphaComponent(0.45).cgColor
        artboardBorder.lineWidth = 1
        artboardTitle.fontSize = 11
        artboardTitle.foregroundColor = NSColor.secondaryLabelColor.cgColor
        layer?.addSublayer(artboardBorder)
        layer?.addSublayer(artboardTitle)
        setAccessibilityLabel("图像画布；滚动或拖动平移，捏合缩放")
        addSubview(stats)
        stats.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            stats.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            stats.topAnchor.constraint(equalTo: topAnchor, constant: 12)
        ])
    }

    required init?(coder: NSCoder) { nil }

    override func makeBackingLayer() -> CALayer {
        let metal = CAMetalLayer()
        metal.colorspace = CGColorSpace(name: CGColorSpace.displayP3)
        metal.isOpaque = true
        return metal
    }

    private var scale: CGFloat { window?.backingScaleFactor ?? 1 }
    private var viewport: CanvasViewport {
        CanvasViewport(width: UInt32(max(0, bounds.width * scale)),
                       height: UInt32(max(0, bounds.height * scale)), scale: Float(scale))
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        shutdown()
        guard window != nil, let metal = layer as? CAMetalLayer else { return }
        metal.contentsScale = scale
        do {
            let pointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(metal).toOpaque()))
            try engine.attachCanvas(metalLayerPtr: pointer, viewport: viewport)
            attached = true
            updateCanvasAppearance()
            let link = displayLink(target: self, selector: #selector(displayFrame(_:)))
            link.add(to: .main, forMode: .common)
            frameLink = link
            markDirty()
        } catch { onError?(error) }
    }

    /// Must run before the view/window releases its layer. Stops callbacks first.
    func shutdown() {
        benchmark?.cancel(BenchmarkFailure("Canvas detached before benchmark completed"))
        frameLink?.invalidate()
        frameLink = nil
        if attached {
            do { try engine.detachCanvas() } catch { onError?(error) }
            attached = false
        }
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        resizeCanvas()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        layer?.contentsScale = scale
        resizeCanvas()
    }
    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance(); updateCanvasAppearance()
    }
    private func updateCanvasAppearance() {
        guard attached else { return }
        effectiveAppearance.performAsCurrentDrawingAppearance {
            if let color = WorkspaceColors.canvas.usingColorSpace(.sRGB) {
                try? engine.setCanvasBackground(red: UInt8((color.redComponent * 255).rounded()), green: UInt8((color.greenComponent * 255).rounded()), blue: UInt8((color.blueComponent * 255).rounded()))
            }
        }
        artboardTitle.foregroundColor = WorkspaceColors.secondary.cgColor
        artboardBorder.strokeColor = WorkspaceColors.secondary.withAlphaComponent(0.5).cgColor
        markDirty()
    }

    private func resizeCanvas() {
        guard attached else { return }
        do { try engine.resizeCanvas(viewport: viewport); markDirty() }
        catch { onError?(error) }
    }

    func markDirty() {
        // A view display link pauses while the window is occluded. Keep the
        // invalidation pending so the first visible callback presents it.
        dirty = true
    }

    func cancelBenchmarkForDocumentChange() {
        benchmark?.cancel(BenchmarkFailure("Document changed during benchmark"))
    }

    func visibilityChanged() {
        if benchmark?.hasStarted == true, window?.occlusionState.contains(.visible) != true {
            benchmark?.cancel(BenchmarkFailure("Window occluded during benchmark"))
        }
        markDirty()
    }

    func startBenchmark(options: BenchmarkOptions, info: ImageInfo, openMs: Double,
                        completion: @escaping (Result<URL, Error>) -> Void) throws {
        guard attached, let screen = window?.screen else { throw BenchmarkFailure("Canvas not attached") }
        let rate = Float(FrameMeasurements.callbackRate(maximumFPS: screen.maximumFramesPerSecond))
        frameLink?.preferredFrameRateRange = CAFrameRateRange(minimum: min(60, rate), maximum: rate, preferred: rate)
        benchmark = CanvasBenchmark(options: options, engine: engine, info: info, viewport: viewport,
                                    screen: screen, openMs: openMs, completion: completion)
        benchmarkLog("STRATA_BENCHMARK_INSTALLED attached=\(attached) paused=\(frameLink?.isPaused ?? true) visible=\(window?.isVisible ?? false) minimized=\(window?.isMiniaturized ?? false) occlusion=\(window?.occlusionState.rawValue ?? 0)")
        markDirty()
    }

    @objc private func displayFrame(_ link: CADisplayLink) {
        let benchmarking = benchmark?.finished == false
        guard attached, dirty || benchmarking else { return }
        do {
            if benchmarking { try benchmark?.beforeFrame(viewport: viewport, window: window, link: link) }
            let frame = try engine.renderFrame()
            if benchmarking { benchmark?.afterFrame(frame) }
            if !frame.skipped {
                updateArtboardOverlay()
                dirty = false
                stats.record(timestamp: link.timestamp, cpuMs: frame.cpuMs)
            }
        } catch {
            // Avoid a repeated modal error on every display tick.
            dirty = false
            if benchmarking { benchmark?.cancel(error) } else { onError?(error) }
        }
    }

    private func updateArtboardOverlay() {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let overlay = engine.artboardOverlay()
        onViewChanged?(overlay, Double(engine.canvasZoom()), scale)
        guard let frame = overlay else {
            artboardBorder.isHidden = true; artboardTitle.isHidden = true; return
        }
        artboardBorder.isHidden = false; artboardTitle.isHidden = false
        let rect = CGRect(x: CGFloat(frame.x) / scale, y: CGFloat(frame.y) / scale,
                          width: CGFloat(frame.width) / scale, height: CGFloat(frame.height) / scale)
        artboardBorder.path = CGPath(rect: rect, transform: nil)
        artboardTitle.string = frame.name
        artboardTitle.contentsScale = scale
        artboardTitle.frame = CGRect(x: rect.minX, y: rect.minY - 18, width: max(140, rect.width), height: 16)
    }

    override func scrollWheel(with event: NSEvent) {
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        // AppKit supplies momentum events; do not add a second inertia simulation.
        pan(dx: event.scrollingDeltaX, dy: event.scrollingDeltaY)
    }

    override func magnify(with event: NSEvent) {
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        let point = convert(event.locationInWindow, from: nil)
        zoom(at: point, factor: 1 + event.magnification)
    }

    override func mouseDown(with event: NSEvent) {
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        window?.makeFirstResponder(self)
        if zoomTool {
            zoom(at: convert(event.locationInWindow, from: nil), factor: event.modifierFlags.contains(.option) ? 0.8 : 1.25)
            return
        }
        lastDrag = convert(event.locationInWindow, from: nil)
        if movesLayer, isEditingEnabled, let layer = activeLayer, let point = lastDrag {
            do { try engine.beginLayerMove(layerId: layer.id); layerDrag = (point, layer.x, layer.y, CGFloat(engine.canvasZoom())); layerPosition = (layer.x, layer.y) }
            catch { onError?(error); lastDrag = nil }
        }
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if let drag = layerDrag {
            guard let dx = Int64(exactly: ((point.x - drag.point.x) * scale / drag.zoom).rounded()),
                  let dy = Int64(exactly: ((point.y - drag.point.y) * scale / drag.zoom).rounded()) else { cancelLayerDrag(); return }
            let (x, xOverflow) = drag.x.addingReportingOverflow(dx)
            let (y, yOverflow) = drag.y.addingReportingOverflow(dy)
            guard !xOverflow, !yOverflow else { cancelLayerDrag(); return }
            do { try engine.previewLayerMove(x: x, y: y); layerPosition = (x, y); markDirty() }
            catch { cancelLayerDrag(); onError?(error) }
            return
        }
        if let previous = lastDrag { pan(dx: point.x - previous.x, dy: point.y - previous.y) }
        lastDrag = point
    }

    override func mouseUp(with event: NSEvent) {
        defer { lastDrag = nil; layerDrag = nil; layerPosition = nil }
        guard layerDrag != nil, let position = layerPosition else { return }
        do { try engine.endLayerMove(x: position.x, y: position.y, cancel: false); markDirty(); onCommitted?() }
        catch { onError?(error) }
    }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { cancelLayerDrag() }
        else if !event.modifierFlags.contains(.command), !event.modifierFlags.contains(.control),
                let key = event.charactersIgnoringModifiers?.uppercased(), onToolKey?(key) == true { return }
        else { super.keyDown(with: event) }
    }
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let pointerTracking { removeTrackingArea(pointerTracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area); pointerTracking = area
    }
    override func mouseMoved(with event: NSEvent) { onPointerMoved?(convert(event.locationInWindow, from: nil)) }
    func panByPoints(dx: CGFloat, dy: CGFloat) { pan(dx: dx, dy: dy) }
    func actualSize() { zoomBy(1 / CGFloat(engine.canvasZoom())) }
    private func cancelLayerDrag() {
        guard layerDrag != nil else { return }
        do { try engine.endLayerMove(x: 0, y: 0, cancel: true) } catch { onError?(error) }
        layerDrag = nil; layerPosition = nil; lastDrag = nil; markDirty()
    }

    private func pan(dx: CGFloat, dy: CGFloat) {
        do { try engine.panBy(dx: Float(dx * scale), dy: Float(dy * scale)); markDirty() }
        catch { onError?(error) }
    }

    private func zoom(at point: NSPoint, factor: CGFloat) {
        do {
            try engine.zoomAt(x: Float(point.x * scale), y: Float(point.y * scale), factor: Float(factor))
            markDirty()
        } catch { onError?(error) }
    }

    func zoomBy(_ factor: CGFloat) {
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        zoom(at: NSPoint(x: bounds.midX, y: bounds.midY), factor: factor)
    }
    func fit() {
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        do { try engine.fitToScreen(); markDirty() } catch { onError?(error) }
    }
}
