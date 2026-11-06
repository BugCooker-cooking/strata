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
    private let selectionBase = CAShapeLayer()
    private let selectionAnts = CAShapeLayer()
    private let marqueePreview = CAShapeLayer()
    private let foregroundPreview = CAShapeLayer()
    private let backgroundPreview = CAShapeLayer()
    private let brushPreview = CAShapeLayer()
    private var selectionEdges: [SelectionEdge] = []
    private lazy var selectionInput = CanvasSelectionInput(engine: engine)
    private lazy var brushInput = CanvasBrushInput(engine: engine)
    private lazy var paintInput = CanvasPaintInput(engine:engine)
    private var paintTool = false
    private let paintGuide = CAShapeLayer()
    private var brushTool = false
    private var brushPreviewColor = NSColor.black
    private var cloneTool = false
    private var cloneSource:NSPoint?
    private let cloneMarker=CAShapeLayer()
    private let cropPreview=CAShapeLayer()
    private let cropHandles=CAShapeLayer()
    lazy var cropInput=CanvasCropInput(engine:engine)
    private var cropTool=false
    lazy var straightenInput=CanvasStraightenInput(engine:engine)
    private var straightenTool=false
    private let straightenGuide=CAShapeLayer()
    private let straightenEndpoints=CAShapeLayer()
    var documentOrigin = NSPoint.zero
    private var selectionCommand: String?
    var isSelectionEnabled = true {
        didSet { if !isSelectionEnabled, !selectionInput.running { selectionInput.cancel() } }
    }
    var isWorkBusy = false
    var onSelectionRunning: ((Bool) -> Void)?
    var onBrushRunning: ((Bool) -> Void)?
    var onColorSample:(([Double],Bool,String)->Void)?
    var onCloneSource:((NSPoint)->Void)?
    private var benchmark: CanvasBenchmark?
    var onError: ((Error) -> Void)?
    var onCommitted: (() -> Void)?
    var onViewChanged: ((ArtboardOverlay?, Double, CGFloat) -> Void)?
    var onPointerMoved: ((NSPoint) -> Void)?
    var onToolKey: ((String) -> Bool)?
    var zoomTool = false
    private var pointerTracking: NSTrackingArea?
    var movesLayer = false
    var isEditingEnabled = true { didSet { if !isEditingEnabled { if !brushInput.running {brushInput.cancel()};if !paintInput.running {paintInput.cancel()};if !cropInput.running{cropInput.cancel()};if !straightenInput.running{straightenInput.cancel()} } } }
    var reportNextRestoredFrame = false
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
        for overlay in [selectionBase, selectionAnts, marqueePreview] {
            overlay.fillColor = nil; overlay.lineWidth = 1; layer?.addSublayer(overlay)
        }
        selectionBase.strokeColor = NSColor.white.cgColor
        selectionAnts.strokeColor = NSColor.black.cgColor; selectionAnts.lineDashPattern = [4, 4]
        marqueePreview.strokeColor = NSColor.white.cgColor; marqueePreview.lineDashPattern = [4, 4]
        for (preview, color) in [(foregroundPreview, NSColor.systemGreen), (backgroundPreview, NSColor.systemRed)] {
            preview.fillColor = color.withAlphaComponent(0.3).cgColor
            layer?.addSublayer(preview)
        }
        brushPreview.fillColor = nil; brushPreview.lineCap = .round; brushPreview.lineJoin = .round
        layer?.addSublayer(brushPreview)
        paintGuide.fillColor=nil;paintGuide.strokeColor=NSColor.white.cgColor;paintGuide.lineWidth=1;paintGuide.lineDashPattern=[4,4];layer?.addSublayer(paintGuide)
        cloneMarker.fillColor=nil;cloneMarker.strokeColor=NSColor.white.cgColor;cloneMarker.lineWidth=1;layer?.addSublayer(cloneMarker)
        cropPreview.fillColor=nil;cropPreview.strokeColor=NSColor.systemBlue.cgColor;cropPreview.lineWidth=1.5;cropPreview.lineDashPattern=[8,4];layer?.addSublayer(cropPreview)
        cropHandles.fillColor=NSColor.white.cgColor;cropHandles.strokeColor=NSColor.systemBlue.cgColor;cropHandles.lineWidth=1;layer?.addSublayer(cropHandles)
        cropInput.onChanged={[weak self] in self?.drawCropPreview();self?.markDirty()}
        cropInput.onRunning={[weak self] running in self?.onBrushRunning?(running)}
        cropInput.onCommitted={[weak self] in self?.markDirty();self?.onCommitted?()}
        cropInput.onError={[weak self] error in self?.onError?(error)}
        straightenGuide.fillColor=nil;straightenGuide.strokeColor=NSColor.systemYellow.cgColor;straightenGuide.lineWidth=1.5;straightenGuide.lineDashPattern=[6,3];layer?.addSublayer(straightenGuide)
        straightenEndpoints.fillColor=NSColor.systemYellow.cgColor;straightenEndpoints.strokeColor=NSColor.black.cgColor;straightenEndpoints.lineWidth=1;layer?.addSublayer(straightenEndpoints)
        straightenInput.onChanged={[weak self] in self?.drawStraightenGuide();self?.markDirty()}
        straightenInput.onRunning={[weak self] running in self?.onBrushRunning?(running)}
        straightenInput.onCommitted={[weak self] in self?.markDirty();self?.onCommitted?()}
        straightenInput.onError={[weak self] error in self?.onError?(error)}
        selectionInput.onChanged = { [weak self] in self?.drawMarqueePreview() }
        selectionInput.onRunning = { [weak self] running in self?.onSelectionRunning?(running) }
        selectionInput.onCommitted = { [weak self] in self?.markDirty(); self?.onCommitted?() }
        selectionInput.onError = { [weak self] error in self?.onError?(error) }
        brushInput.onChanged = { [weak self] in self?.drawBrushPreview() }
        brushInput.onRunning = { [weak self] running in self?.onBrushRunning?(running) }
        brushInput.onCommitted = { [weak self] in self?.markDirty(); self?.onCommitted?() }
        brushInput.onError = { [weak self] error in self?.onError?(error) }
        paintInput.onChanged = {[weak self] in self?.drawPaintGuide()}
        paintInput.onRunning = {[weak self] running in self?.onBrushRunning?(running)}
        paintInput.onCommitted = {[weak self] in self?.markDirty();self?.onCommitted?()}
        paintInput.onSample = {[weak self] color,background,profile in self?.onColorSample?(color,background,profile)}
        paintInput.onError = {[weak self] error in self?.onError?(error)}
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
        cancelMarquee()
        brushInput.cancel()
        paintInput.cancel()
        cropInput.cancel()
        straightenInput.cancel()
        selectionAnts.removeAllAnimations()
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
        guard attached, !isWorkBusy || benchmarking, dirty || benchmarking else { return }
        do {
            if benchmarking { try benchmark?.beforeFrame(viewport: viewport, window: window, link: link) }
            let frame = try engine.renderFrame()
            if benchmarking { benchmark?.afterFrame(frame) }
            if !frame.skipped {
                updateArtboardOverlay()
                if reportNextRestoredFrame {
                    reportNextRestoredFrame = false
                    benchmarkLog("STRATA_WINDOW restored_frame=true minimized=\(window?.isMiniaturized ?? true) zoom=\(engine.canvasZoom())")
                }
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
            artboardBorder.isHidden = true; artboardTitle.isHidden = true; selectionBase.isHidden = true; selectionAnts.isHidden = true; return
        }
        artboardBorder.isHidden = false; artboardTitle.isHidden = false
        let rect = CGRect(x: CGFloat(frame.x) / scale, y: CGFloat(frame.y) / scale,
                          width: CGFloat(frame.width) / scale, height: CGFloat(frame.height) / scale)
        artboardBorder.path = CGPath(rect: rect, transform: nil)
        artboardTitle.string = frame.name
        artboardTitle.contentsScale = scale
        artboardTitle.frame = CGRect(x: rect.minX, y: rect.minY - 18, width: max(140, rect.width), height: 16)
        let zoom = CGFloat(engine.canvasZoom()) / scale
        let path = CGMutablePath()
        for edge in selectionEdges {
            path.move(to: CGPoint(x: rect.minX + CGFloat(edge.x1) * zoom, y: rect.minY + CGFloat(edge.y1) * zoom))
            path.addLine(to: CGPoint(x: rect.minX + CGFloat(edge.x2) * zoom, y: rect.minY + CGFloat(edge.y2) * zoom))
        }
        selectionBase.path = path; selectionAnts.path = path
        selectionBase.isHidden = selectionEdges.isEmpty; selectionAnts.isHidden = selectionEdges.isEmpty
        drawMarqueePreview()
        drawBrushPreview()
        drawPaintGuide()
        drawCloneMarker()
        drawCropPreview()
        drawStraightenGuide()
    }
    func setSelectionOutline(_ edges: [SelectionEdge]) {
        selectionEdges = edges
        if !edges.isEmpty, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            if selectionAnts.animation(forKey: "marching") == nil {
                let animation = CABasicAnimation(keyPath: "lineDashPhase")
                animation.fromValue = 0; animation.toValue = 8; animation.duration = 0.8; animation.repeatCount = .infinity
                selectionAnts.add(animation, forKey: "marching")
            }
        } else { selectionAnts.removeAllAnimations() }
        markDirty()
    }
    private func documentPoint(_ point: NSPoint) -> NSPoint? {
        guard let frame = engine.artboardOverlay() else { return nil }
        let zoom = CGFloat(engine.canvasZoom())
        guard zoom > 0 else { return nil }
        return NSPoint(x: documentOrigin.x + (point.x * scale - CGFloat(frame.x)) / zoom,
                       y: documentOrigin.y + (point.y * scale - CGFloat(frame.y)) / zoom)
    }
    private func drawMarqueePreview() {
        guard let frame = engine.artboardOverlay() else { return }
        let zoom = CGFloat(engine.canvasZoom()) / scale
        var transform = CGAffineTransform(a: zoom, b: 0, c: 0, d: zoom,
            tx: CGFloat(frame.x) / scale - documentOrigin.x * zoom, ty: CGFloat(frame.y) / scale - documentOrigin.y * zoom)
        let paths = selectionInput.paths()
        CATransaction.begin(); CATransaction.setDisableActions(true)
        marqueePreview.path = paths.0?.copy(using: &transform)
        foregroundPreview.path = paths.1?.copy(using: &transform)
        backgroundPreview.path = paths.2?.copy(using: &transform)
        CATransaction.commit()
    }
    private func cancelMarquee() {
        selectionInput.cancel(); lastDrag = nil
    }
    private func drawCropPreview(){
        guard let path=cropInput.path(),let draft=cropInput.draft,let frame=engine.artboardOverlay()else{cropPreview.path=nil;cropHandles.path=nil;return}
        let zoom=CGFloat(engine.canvasZoom())/scale
        var transform=CGAffineTransform(a:zoom,b:0,c:0,d:zoom,tx:CGFloat(frame.x)/scale-documentOrigin.x*zoom,ty:CGFloat(frame.y)/scale-documentOrigin.y*zoom)
        let handles=CGMutablePath();for handle in CropHandle.allCases{let point=handle.position(in:draft).applying(transform);handles.addRect(CGRect(x:point.x-3,y:point.y-3,width:6,height:6))}
        CATransaction.begin();CATransaction.setDisableActions(true);cropPreview.path=path.copy(using:&transform);cropHandles.path=handles;CATransaction.commit()
    }
    private func drawStraightenGuide(){
        guard let path=straightenInput.path(),let draft=straightenInput.draft,let frame=engine.artboardOverlay()else{straightenGuide.path=nil;straightenEndpoints.path=nil;return}
        let zoom=CGFloat(engine.canvasZoom())/scale
        var transform=CGAffineTransform(a:zoom,b:0,c:0,d:zoom,tx:CGFloat(frame.x)/scale-documentOrigin.x*zoom,ty:CGFloat(frame.y)/scale-documentOrigin.y*zoom)
        let endpoints=CGMutablePath();for p in [draft.start,draft.end]{let point=p.applying(transform);endpoints.addEllipse(in:CGRect(x:point.x-3,y:point.y-3,width:6,height:6))}
        CATransaction.begin();CATransaction.setDisableActions(true);straightenGuide.path=path.copy(using:&transform);straightenEndpoints.path=endpoints;CATransaction.commit()
    }
    func configureCrop(_ model:WorkspaceModel){
        let enabled=model.tool == .crop
        if enabled && !cropTool && !straightenTool{cancelLayerDrag()}
        cropTool=enabled && model.cropMode=="crop";straightenTool=enabled && model.cropMode=="straighten"
        cropInput.configure(enabled:cropTool,ratio:model.cropRatio,deletePixels:model.cropDeletesPixels)
        straightenInput.configure(enabled:straightenTool,align:model.straightenAlign,autoCrop:model.straightenAutoCrop,interpolation:model.straightenInterpolation)
    }
    func configureSelection(_ model: WorkspaceModel) {
        let command: String?
        switch model.tool {
        case .marquee: command = model.marqueeShape == "ellipse" ? "select.ellipse" : "select.marquee"
        case .lasso: command = model.lassoShape == "polygon" ? "select.polygon" : "select.lasso"
        case .selection: command = model.selectionShape == "wand" ? "select.wand" : "select.quick"
        default: command = nil
        }
        if command == nil { selectionInput.cancel() }
        selectionCommand = command
        func bounded(_ value: Double, _ range: ClosedRange<Double>, fallback: Double) -> Double {
            value.isFinite ? min(range.upperBound, max(range.lowerBound, value)) : fallback
        }
        selectionInput.configure(CanvasSelectionConfiguration(command: command ?? "select.marquee", mode: model.selectionMode,
            sample: model.selectionSample, tolerance: Int(bounded(model.wandTolerance, 0...255, fallback: 32)), contiguous: model.wandContiguous,
            radius: bounded(model.quickRadius, 0.5...256, fallback: 8), smoothness: bounded(model.quickSmoothness, 0...100, fallback: 8)))
        selectionInput.paintBackground = model.quickBackground
    }
    func configureBrush(_ model: WorkspaceModel) {
        brushTool = model.tool.isBrush
        let spot=model.tool == .heal && model.healMode == "spot"
        let tonal=model.tool == .dodge
        cloneTool = model.tool == .clone || model.tool == .heal && !spot
        cloneSource = cloneTool && model.cloneSource?.count == 2 ? NSPoint(x:model.cloneSource?[0] ?? 0,y:model.cloneSource?[1] ?? 0) : nil
        drawCloneMarker()
        if !brushTool { brushInput.cancel(); return }
        let color = model.foregroundColor.usingColorSpace(model.workingColorSpace) ?? NSColor(srgbRed:0,green:0,blue:0,alpha:1)
        func bounded(_ value: Double, _ range: ClosedRange<Double>, _ fallback: Double) -> Double {
            value.isFinite ? min(range.upperBound,max(range.lowerBound,value)) : fallback
        }
        let rgba = [Double(color.redComponent),Double(color.greenComponent),Double(color.blueComponent),Double(color.alphaComponent)].map { bounded($0,0...1,0) }
        let maskColor = (model.tool == .eraser ? model.backgroundColor : model.foregroundColor).usingColorSpace(model.workingColorSpace) ?? NSColor(srgbRed:1,green:1,blue:1,alpha:1)
        let maskValue = bounded(Double(maskColor.redComponent)*0.2126+Double(maskColor.greenComponent)*0.7152+Double(maskColor.blueComponent)*0.0722,0...1,1)
        brushPreviewColor = model.tool == .eraser ? NSColor.white.withAlphaComponent(0.5) : color
        let configuration = CanvasBrushConfiguration(layer:model.selected?.id,erase:model.tool == .eraser,target:cloneTool || spot || tonal ? "pixels" : model.brushTarget,
            size:bounded(model.brushSize,0.5...4096,24),hardness:bounded(model.brushHardness/100,0...1,0.8),
            opacity:bounded(model.brushOpacity/100,0...1,1),flow:bounded(model.brushFlow/100,0...1,1),
            spacing:bounded(model.brushSpacing/100,0.01...2,0.2),color:rgba,
            value:maskValue,pressureSize:model.brushPressureSize,pressureOpacity:model.brushPressureOpacity,clone:cloneTool,aligned:model.cloneAligned,cloneSource:model.cloneSource,cloneSample:model.cloneSample,heal:model.tool == .heal && !spot,spot:spot,spotSearchRadius:bounded(model.spotSearchRadius,1...512,64),
            tonalCommand:tonal ? "retouch."+model.tonalBrushMode : nil,tonalRange:model.tonalBrushRange,tonalExposure:bounded(model.tonalBrushExposure/100,0...1,0.5),protectTones:model.tonalBrushProtect,spongeMode:model.spongeMode,spongeVibrance:model.spongeVibrance)
        brushInput.configure(configuration)
    }
    func configurePaint(_ model:WorkspaceModel){
        paintTool=[EditorTool.gradient,.bucket,.eyedropper].contains(model.tool)
        if !paintTool{paintInput.cancel();return}
        func rgba(_ color:NSColor)->[Double]{let c=color.usingColorSpace(model.workingColorSpace) ?? NSColor(srgbRed:0,green:0,blue:0,alpha:1);return [c.redComponent,c.greenComponent,c.blueComponent,c.alphaComponent].map{min(1,max(0,Double($0)))}}
        func integer(_ value:Double,max:Int,fallback:Int)->Int{value.isFinite ? min(max,Swift.max(0,Int(min(Double(max),Swift.max(0,value))))) : fallback}
        let command=model.tool == .eyedropper ? "paint.sample_color" : model.tool == .bucket ? "paint.bucket" : "paint.gradient"
        paintInput.configure(CanvasPaintConfiguration(command:command,layer:model.selected?.id,revision:model.snapshot?.revision ?? 0,
            foreground:rgba(model.foregroundColor),background:rgba(model.backgroundColor),opacity:model.paintOpacity.isFinite ? min(1,max(0,model.paintOpacity/100)) : 1,
            tolerance:integer(model.bucketTolerance,max:255,fallback:32),contiguous:model.bucketContiguous,sample:model.paintSample,radius:integer(model.sampleRadius,max:31,fallback:0),shape:model.gradientShape,reverse:model.gradientReverse))
    }
    private func drawPaintGuide(){
        guard let path=paintInput.path(),let frame=engine.artboardOverlay()else{paintGuide.path=nil;return}
        let zoom=CGFloat(engine.canvasZoom())/scale;var transform=CGAffineTransform(a:zoom,b:0,c:0,d:zoom,tx:CGFloat(frame.x)/scale-documentOrigin.x*zoom,ty:CGFloat(frame.y)/scale-documentOrigin.y*zoom)
        paintGuide.path=path.copy(using:&transform)
    }
    private func drawCloneMarker(){
        guard cloneTool,let source=cloneSource,let frame=engine.artboardOverlay()else{cloneMarker.path=nil;return}
        let zoom=CGFloat(engine.canvasZoom())/scale;let p=NSPoint(x:CGFloat(frame.x)/scale+(source.x-documentOrigin.x)*zoom,y:CGFloat(frame.y)/scale+(source.y-documentOrigin.y)*zoom)
        let path=CGMutablePath();path.addEllipse(in:CGRect(x:p.x-5,y:p.y-5,width:10,height:10));path.move(to:NSPoint(x:p.x-9,y:p.y));path.addLine(to:NSPoint(x:p.x+9,y:p.y));path.move(to:NSPoint(x:p.x,y:p.y-9));path.addLine(to:NSPoint(x:p.x,y:p.y+9));cloneMarker.path=path
    }
    private func drawBrushPreview() {
        guard let source = brushInput.path(), let frame = engine.artboardOverlay() else { brushPreview.path = nil; return }
        let zoom = CGFloat(engine.canvasZoom())/scale
        var transform = CGAffineTransform(a:zoom,b:0,c:0,d:zoom,tx:CGFloat(frame.x)/scale-documentOrigin.x*zoom,ty:CGFloat(frame.y)/scale-documentOrigin.y*zoom)
        CATransaction.begin();CATransaction.setDisableActions(true)
        brushPreview.path = source.copy(using:&transform)
        brushPreview.lineWidth = brushInput.configuration.size*zoom
        brushPreview.strokeColor = brushPreviewColor.withAlphaComponent(brushInput.configuration.opacity*0.6).cgColor
        CATransaction.commit()
    }
    private func brushPressure(_ event: NSEvent) -> Double {
        event.subtype == .tabletPoint ? min(1,max(0,Double(event.pressure))) : 1
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
        guard !isWorkBusy else { return }
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        window?.makeFirstResponder(self)
        if straightenTool{guard isEditingEnabled,let point=documentPoint(convert(event.locationInWindow,from:nil))else{return};straightenInput.down(point);return}
        if cropTool{guard isEditingEnabled,let point=documentPoint(convert(event.locationInWindow,from:nil))else{return};let zoom=CGFloat(engine.canvasZoom())/scale;cropInput.down(point,hitTolerance:8/max(zoom,0.0001));return}
        if paintTool{guard let point=documentPoint(convert(event.locationInWindow,from:nil))else{return};paintInput.down(point,background:event.modifierFlags.contains(.option));return}
        if brushTool {
            guard isEditingEnabled, let point = documentPoint(convert(event.locationInWindow,from:nil)) else { return }
            if cloneTool,event.modifierFlags.contains(.option){onCloneSource?(point);return}
            brushInput.down(point,pressure:brushPressure(event)); return
        }
        if selectionCommand != nil {
            guard isSelectionEnabled, let point = documentPoint(convert(event.locationInWindow, from: nil)) else { return }
            selectionInput.down(point, modifiers: event.modifierFlags, clicks: event.clickCount)
            return
        }
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
        if straightenTool{if let document=documentPoint(point){straightenInput.drag(document)};return}
        if cropTool{if let document=documentPoint(point){cropInput.drag(document)};return}
        if paintTool{if let document=documentPoint(point){paintInput.drag(document)};return}
        if brushTool { if let point = documentPoint(point) { brushInput.drag(point,pressure:brushPressure(event)) }; return }
        if selectionCommand != nil {
            if let current = documentPoint(point) { selectionInput.drag(current) }
            return
        }
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
        if straightenTool{if let point=documentPoint(convert(event.locationInWindow,from:nil)){straightenInput.up(point)};return}
        if cropTool{if let point=documentPoint(convert(event.locationInWindow,from:nil)){cropInput.up(point)};return}
        if paintTool{if let point=documentPoint(convert(event.locationInWindow,from:nil)){paintInput.up(point)};return}
        if brushTool { if let point = documentPoint(convert(event.locationInWindow,from:nil)) { brushInput.up(point,pressure:brushPressure(event)) }; return }
        if selectionCommand != nil {
            if let point = documentPoint(convert(event.locationInWindow, from: nil)) { selectionInput.up(point) }
            return
        }
        defer { lastDrag = nil; layerDrag = nil; layerPosition = nil }
        guard layerDrag != nil, let position = layerPosition else { return }
        do { try engine.endLayerMove(x: position.x, y: position.y, cancel: false); markDirty(); onCommitted?() }
        catch { onError?(error) }
    }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { cancelLayerDrag(); cancelMarquee(); brushInput.cancel();paintInput.cancel();cropInput.cancel();straightenInput.cancel() }
        else if event.keyCode == 36,straightenInput.confirm(){return}
        else if event.keyCode == 36,cropInput.confirm(){return}
        else if event.keyCode == 36, selectionInput.confirm() { return }
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
    override func mouseMoved(with event: NSEvent) {
        guard !isWorkBusy else { return }
        let point = convert(event.locationInWindow, from: nil)
        if selectionCommand != nil, isSelectionEnabled, let document = documentPoint(point) { selectionInput.hover(document) }
        onPointerMoved?(point)
    }
    func panByPoints(dx: CGFloat, dy: CGFloat) { pan(dx: dx, dy: dy) }
    func actualSize() {
        guard !isWorkBusy else{return}
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        do{_ = try engine.dispatch(command:"view.actual_size",paramsJson:"{}");markDirty()}catch{onError?(error)}
    }
    private func cancelLayerDrag() {
        guard layerDrag != nil else { return }
        do { try engine.endLayerMove(x: 0, y: 0, cancel: true) } catch { onError?(error) }
        layerDrag = nil; layerPosition = nil; lastDrag = nil; markDirty()
    }

    private func pan(dx: CGFloat, dy: CGFloat) {
        guard !isWorkBusy else { return }
        do { try engine.panBy(dx: Float(dx * scale), dy: Float(dy * scale)); markDirty() }
        catch { onError?(error) }
    }

    private func zoom(at point: NSPoint, factor: CGFloat) {
        guard !isWorkBusy else { return }
        do {
            try engine.zoomAt(x: Float(point.x * scale), y: Float(point.y * scale), factor: Float(factor))
            markDirty()
        } catch { onError?(error) }
    }

    func zoomBy(_ factor: CGFloat) {
        guard !isWorkBusy else { return }
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        zoom(at: NSPoint(x: bounds.midX, y: bounds.midY), factor: factor)
    }
    func fit() {
        guard !isWorkBusy else { return }
        benchmark?.cancel(BenchmarkFailure("User input interrupted benchmark"))
        do { try engine.fitToScreen(); markDirty() } catch { onError?(error) }
    }
}
