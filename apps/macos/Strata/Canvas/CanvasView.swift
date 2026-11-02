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
    private var benchmark: CanvasBenchmark?
    var onError: ((Error) -> Void)?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    init(engine: StrataEngine) {
        self.engine = engine
        super.init(frame: .zero)
        wantsLayer = true
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

    private func resizeCanvas() {
        guard attached else { return }
        do { try engine.resizeCanvas(viewport: viewport); markDirty() }
        catch { onError?(error) }
    }

    func markDirty() { dirty = true }

    func cancelBenchmarkForDocumentChange() {
        benchmark?.cancel(BenchmarkFailure("Document changed during benchmark"))
    }

    func startBenchmark(options: BenchmarkOptions, info: ImageInfo, openMs: Double,
                        completion: @escaping (Result<URL, Error>) -> Void) throws {
        guard attached, let screen = window?.screen else { throw BenchmarkFailure("Canvas not attached") }
        benchmark = CanvasBenchmark(options: options, engine: engine, info: info, viewport: viewport,
                                    screen: screen, openMs: openMs, completion: completion)
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
                dirty = false
                stats.record(timestamp: link.timestamp, cpuMs: frame.cpuMs)
            }
        } catch {
            // Avoid a repeated modal error on every display tick.
            dirty = false
            if benchmarking { benchmark?.cancel(error) } else { onError?(error) }
        }
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
        lastDrag = convert(event.locationInWindow, from: nil)
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if let previous = lastDrag { pan(dx: point.x - previous.x, dy: point.y - previous.y) }
        lastDrag = point
    }

    override func mouseUp(with event: NSEvent) { lastDrag = nil }

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
