import AppKit
import SwiftUI
import Combine

@MainActor
final class CanvasChromeView: NSView {
    let canvas: CanvasView
    let readout = CanvasReadout()
    private let horizontal = CanvasRuler(vertical: false)
    private let vertical = CanvasRuler(vertical: true)
    private let rulerCorner=CanvasRulerCorner()
    private let map = CanvasNavigator()
    private var frameOverlay: ArtboardOverlay?
    private var controlSubscription: AnyCancellable?
    private var canvasLeading:NSLayoutConstraint!
    private var canvasTop:NSLayoutConstraint!
    private var documentOrigin=NSPoint.zero
    private var rulerMetrics:EditorSnapshot.RulerMetrics?
    override var isFlipped: Bool { true }

    init(canvas: CanvasView, model: WorkspaceModel) {
        self.canvas = canvas
        super.init(frame: .zero)
        let zoom = NSHostingView(rootView: CanvasZoomBar(model: model, readout: readout))
        let empty = NSHostingView(rootView: CanvasEmptyState(model: model))
        let banner = NSHostingView(rootView: CanvasControlBanner(model: model)); banner.isHidden = true
        wantsLayer = true
        for view in [canvas, horizontal, vertical, rulerCorner, zoom, map, empty, banner] { addSubview(view); view.translatesAutoresizingMaskIntoConstraints = false }
        canvasLeading=canvas.leadingAnchor.constraint(equalTo:leadingAnchor,constant:18)
        canvasTop=canvas.topAnchor.constraint(equalTo:topAnchor,constant:18)
        NSLayoutConstraint.activate([
            empty.centerXAnchor.constraint(equalTo: canvas.centerXAnchor), empty.centerYAnchor.constraint(equalTo: canvas.centerYAnchor),
            canvasLeading,canvasTop,
            canvas.trailingAnchor.constraint(equalTo: trailingAnchor), canvas.bottomAnchor.constraint(equalTo: bottomAnchor),
            horizontal.leadingAnchor.constraint(equalTo: canvas.leadingAnchor), horizontal.trailingAnchor.constraint(equalTo: trailingAnchor), horizontal.topAnchor.constraint(equalTo: topAnchor), horizontal.heightAnchor.constraint(equalToConstant: 18),
            vertical.leadingAnchor.constraint(equalTo: leadingAnchor), vertical.topAnchor.constraint(equalTo: canvas.topAnchor), vertical.bottomAnchor.constraint(equalTo: bottomAnchor), vertical.widthAnchor.constraint(equalToConstant: 18),
            rulerCorner.leadingAnchor.constraint(equalTo:leadingAnchor),rulerCorner.topAnchor.constraint(equalTo:topAnchor),rulerCorner.widthAnchor.constraint(equalToConstant:18),rulerCorner.heightAnchor.constraint(equalToConstant:18),
            zoom.leadingAnchor.constraint(equalTo: canvas.leadingAnchor, constant: 12), zoom.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -12),
            map.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12), map.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -12), map.widthAnchor.constraint(equalToConstant: 132), map.heightAnchor.constraint(equalToConstant: 88)
            , banner.topAnchor.constraint(equalTo: canvas.topAnchor, constant: 5), banner.leadingAnchor.constraint(equalTo: canvas.leadingAnchor, constant: 5), banner.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -5), banner.heightAnchor.constraint(equalToConstant: 36)
        ])
        controlSubscription = model.$snapshot.sink { [weak self, weak banner, weak model] snapshot in
            let control = snapshot?.control?.kind ?? "user"
            banner?.isHidden = control == "user"
            self?.layer?.borderWidth = control == "user" ? 0 : 2
            self?.layer?.borderColor = (control == "paused" ? WorkspaceColors.warning : WorkspaceColors.ai).cgColor
            guard let self else{return}
            self.documentOrigin=NSPoint(x:Double(snapshot?.frame?.x ?? 0),y:Double(snapshot?.frame?.y ?? 0))
            self.rulerMetrics=snapshot?.rulerMetrics?.valid==true ? snapshot?.rulerMetrics:nil
            let shown=snapshot?.presentation?.rulersVisible ?? true
            self.horizontal.isHidden = !shown;self.vertical.isHidden = !shown;self.rulerCorner.isHidden = !shown
            if let snapshot{self.canvas.rulerOriginInput.bind(snapshot)}else{self.canvas.rulerOriginInput.cancel()}
            let inset:CGFloat=shown ? 18:0
            if self.canvasLeading.constant != inset{self.canvasLeading.constant=inset;self.canvasTop.constant=inset;self.needsLayout=true}
            self.horizontal.onGuide = { [weak self,weak model]phase,event in self?.rulerGuide(phase,event:event,axis:"horizontal",model:model) }
            self.vertical.onGuide = { [weak self,weak model]phase,event in self?.rulerGuide(phase,event:event,axis:"vertical",model:model) }
        }
        rulerCorner.onPointer={[weak self,weak model]phase,event in self?.dragOrigin(phase,event:event,model:model)}
        rulerCorner.onReset={[weak canvas,weak model]in guard model?.busy==false else{return};canvas?.rulerOriginInput.reset();canvas?.window?.makeFirstResponder(canvas)}
        rulerCorner.onCancel={[weak canvas]in canvas?.rulerOriginInput.cancel();canvas?.window?.makeFirstResponder(canvas)}
        canvas.onViewChanged = { [weak self] frame, zoom, scale in
            guard let self else { return }
            self.frameOverlay = frame
            if abs(self.readout.zoom - zoom) > 0.0005 { self.readout.zoom = zoom }
            let origin=self.rulerMetrics?.origin ?? [0,0],units=self.rulerMetrics?.pixelsPerUnit ?? [1,1]
            self.horizontal.origin = CGFloat(frame?.x ?? 0) / scale+(origin[0]-self.documentOrigin.x)*zoom/scale
            self.vertical.origin = CGFloat(frame?.y ?? 0) / scale+(origin[1]-self.documentOrigin.y)*zoom/scale
            self.horizontal.unitScale = zoom / scale*units[0]; self.vertical.unitScale = zoom / scale*units[1]
            self.horizontal.unitSymbol=self.rulerMetrics?.symbol ?? "px";self.vertical.unitSymbol=self.horizontal.unitSymbol
            self.horizontal.needsDisplay = true; self.vertical.needsDisplay = true
            self.map.artboard = frame.map { CGRect(x: CGFloat($0.x) / scale, y: CGFloat($0.y) / scale, width: CGFloat($0.width) / scale, height: CGFloat($0.height) / scale) }
            self.map.viewport = self.canvas.bounds; self.map.needsDisplay = true
        }
        canvas.onPointerMoved = { [weak self] point in
            guard let self, let frame = self.frameOverlay else { return }
            let scale = self.window?.backingScaleFactor ?? 1
            let x = self.documentOrigin.x+(point.x * scale - CGFloat(frame.x)) / self.readout.zoom
            let y = self.documentOrigin.y+(point.y * scale - CGFloat(frame.y)) / self.readout.zoom
            if let metrics=self.rulerMetrics,let value=metrics.rulerPoint(NSPoint(x:x,y:y)){self.readout.coordinates=String(format:"X %.3f  Y %.3f %@",value.x,value.y,metrics.symbol)}
            else{self.readout.coordinates=String(format:"X %.0f  Y %.0f px",x,y)}
        }
        map.onNavigate = { [weak canvas] dx, dy in canvas?.panByPoints(dx: dx, dy: dy) }
    }
    required init?(coder: NSCoder) { nil }
    private func dragOrigin(_ phase:Int,event:NSEvent,model:WorkspaceModel?){
        guard let model,!model.busy else{return}
        let local=canvas.convert(event.locationInWindow,from:nil)
        guard let point=canvas.documentPoint(local)else{canvas.rulerOriginInput.cancel();return}
        if phase==0{canvas.rulerOriginInput.begin(at:point)}
        else if phase==1{canvas.rulerOriginInput.move(to:point)}
        else{canvas.window?.makeFirstResponder(canvas);_ = canvas.rulerOriginInput.finish(at:canvas.bounds.contains(local) ? point:nil)}
    }
    private func rulerGuide(_ phase:Int,event:NSEvent,axis:String,model:WorkspaceModel?){
        guard let model,!model.busy,let point=canvas.documentPoint(canvas.convert(event.locationInWindow,from:nil))else{return}
        let value=Double(axis=="vertical" ? point.x:point.y)
        if phase==0{canvas.rulerOriginInput.cancel();canvas.guideInput.begin(axis:axis,position:value)}
        else if phase==1{canvas.guideInput.move(position:value)}
        else{let local=canvas.convert(event.locationInWindow,from:nil);if canvas.bounds.contains(local){canvas.guideInput.move(position:value);_ = canvas.guideInput.finish()}else{canvas.guideInput.cancel()}}
    }
    override func draw(_ dirtyRect: NSRect) { WorkspaceColors.panel.setFill(); dirtyRect.fill() }
}

@MainActor
final class CanvasRulerCorner:NSView{
    var onPointer:((Int,NSEvent)->Void)?
    var onReset:(()->Void)?
    var onCancel:(()->Void)?
    override var isFlipped:Bool{true}
    override var acceptsFirstResponder:Bool{true}
    override init(frame:NSRect){super.init(frame:frame);toolTip=L10n.text("拖动设置标尺零点；双击重置 / Drag ruler origin; double-click resets");setAccessibilityElement(true);setAccessibilityRole(.button);setAccessibilityLabel(L10n.text("标尺原点：拖动设置，双击重置"))}
    convenience init(){self.init(frame:.zero)}
    required init?(coder:NSCoder){nil}
    override func mouseDown(with event:NSEvent){window?.makeFirstResponder(self);if event.clickCount>=2{onCancel?();onReset?()}else{onPointer?(0,event)}}
    override func mouseDragged(with event:NSEvent){onPointer?(1,event)}
    override func mouseUp(with event:NSEvent){onPointer?(2,event)}
    override func keyDown(with event:NSEvent){if event.keyCode==53{onCancel?()}else{super.keyDown(with:event)}}
    override func accessibilityPerformPress()->Bool{onReset?();return true}
    override func draw(_ dirtyRect:NSRect){
        WorkspaceColors.panel.setFill();bounds.fill();WorkspaceColors.secondary.setStroke()
        let p=NSBezierPath();p.lineWidth=1;p.move(to:NSPoint(x:4,y:9));p.line(to:NSPoint(x:14,y:9));p.move(to:NSPoint(x:9,y:4));p.line(to:NSPoint(x:9,y:14));p.stroke()
        WorkspaceColors.border.setStroke();NSBezierPath(rect:bounds.insetBy(dx:0.5,dy:0.5)).stroke()
    }
}

private struct CanvasControlBanner: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: model.snapshot?.control?.kind == "paused" ? "pause.circle" : "sparkles")
            Text(model.snapshot?.control?.kind == "paused" ? L10n.text("AI 等你决定 · 可撤销与切换显隐") : L10n.text("AI 执行中 · 可缩放平移查看")).lineLimit(1)
            Spacer(minLength: 0)
            Button(L10n.text("停止 ⌘.")) { model.assistant?.stop() }.buttonStyle(.borderless)
        }.font(.system(size: 11)).padding(.horizontal, 10).frame(maxWidth: .infinity, maxHeight: .infinity)
            .foregroundStyle(Color(nsColor: model.snapshot?.control?.kind == "paused" ? WorkspaceColors.warning : WorkspaceColors.ai))
            .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: WorkspaceColors.panel).opacity(0.95)))
    }
}

private struct CanvasEmptyState: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        if !model.hasDocument {
            VStack(spacing: 14) {
                Image(systemName: "photo.on.rectangle.angled").font(.system(size: 28))
                Text(L10n.text("从一张图片开始")).font(.system(size: 17, weight: .medium))
                HStack {
                    Button(L10n.text("打开文件…")) { model.send(.open) }
                    Button(L10n.text("新建文档…")) { model.send(.new) }
                }
            }.padding(28).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                .background(RoundedRectangle(cornerRadius: 12).fill(Color(nsColor: WorkspaceColors.panel)))
        }
    }
}

private struct CanvasZoomBar: View {
    @ObservedObject var model: WorkspaceModel
    @ObservedObject var readout: CanvasReadout
    var body: some View {
        HStack(spacing: 2) {
            WorkspaceIconButton(title: L10n.text("缩小"), symbol: "minus") { model.send(.zoom(0.8)) }
            Button("\(Int((readout.zoom * 100).rounded()))%") { model.send(.actualSize) }.buttonStyle(WorkspaceButtonStyle()).font(.system(size: 11)).monospacedDigit().frame(minWidth: 43).help(L10n.text("实际像素 100%"))
            WorkspaceIconButton(title: L10n.text("放大"), symbol: "plus") { model.send(.zoom(1.25)) }
            WorkspaceIconButton(title: L10n.text("适合屏幕 (⌘0)"), symbol: "viewfinder") { model.send(.fit) }
        }.padding(3).background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: WorkspaceColors.panel)))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(nsColor: WorkspaceColors.border)))
    }
}

@MainActor
final class CanvasRuler: NSView {
    var origin: CGFloat = 0
    var unitScale: CGFloat = 1
    var unitSymbol="px"
    let vertical: Bool
    var onGuide:((Int,NSEvent)->Void)?
    override var isFlipped: Bool { true }
    init(vertical: Bool) { self.vertical = vertical; super.init(frame: .zero); setAccessibilityElement(false) }
    required init?(coder: NSCoder) { nil }
    override func mouseDown(with event:NSEvent){onGuide?(0,event)}
    override func mouseDragged(with event:NSEvent){onGuide?(1,event)}
    override func mouseUp(with event:NSEvent){onGuide?(2,event)}
    override func draw(_ dirtyRect: NSRect) {
        WorkspaceColors.panel.setFill(); bounds.fill()
        guard unitScale > 0, unitScale.isFinite else { return }
        let desired = 70 / unitScale
        let power = pow(10, floor(log10(desired)))
        let step = ([1.0, 2, 5, 10].first { $0 * power >= desired } ?? 10) * power
        let length = vertical ? bounds.height : bounds.width
        let first = floor(-origin / unitScale / step) * step
        let path = NSBezierPath(); path.lineWidth = 0.5
        for index in 0...Int(min(500, ceil(length / (step * unitScale)) + 2)) {
            let value = first + Double(index) * step
            let position = origin + value * unitScale
            for minor in 0..<5 {
                let p = position + Double(minor) * step * unitScale / 5
                path.move(to: vertical ? NSPoint(x: 18, y: p) : NSPoint(x: p, y: 18))
                path.line(to: vertical ? NSPoint(x: minor == 0 ? 10 : 14, y: p) : NSPoint(x: p, y: minor == 0 ? 10 : 14))
            }
            let digits=max(0,min(6,Int(-floor(log10(step)))))
            let display=abs(value)<step*1e-8 ? 0.0:Double(value)
            let text = String(format: "%.\(digits)f",display) as NSString
            let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.monospacedDigitSystemFont(ofSize: 8, weight: .regular), .foregroundColor: WorkspaceColors.muted]
            if vertical {
                NSGraphicsContext.saveGraphicsState()
                var transform = AffineTransform(translationByX: 2, byY: position - 3); transform.rotate(byDegrees: -90); (transform as NSAffineTransform).concat()
                text.draw(at: .zero, withAttributes: attributes); NSGraphicsContext.restoreGraphicsState()
            } else { text.draw(at: NSPoint(x: position + 3, y: 1), withAttributes: attributes) }
        }
        WorkspaceColors.border.setStroke(); path.stroke()
        let badge=NSRect(x:vertical ? 0:max(0,bounds.width-26),y:0,width:vertical ? 18:26,height:14)
        WorkspaceColors.panel.setFill();badge.fill()
        (unitSymbol as NSString).draw(at:NSPoint(x:badge.minX+2,y:1),withAttributes:[.font:NSFont.systemFont(ofSize:8),.foregroundColor:WorkspaceColors.secondary])
    }
}

@MainActor
private final class CanvasNavigator: NSView {
    var artboard: CGRect?
    var viewport = CGRect.zero
    var onNavigate: ((CGFloat, CGFloat) -> Void)?
    private var mapScale: CGFloat = 1
    private var mapOrigin = CGPoint.zero
    override var isFlipped: Bool { true }
    override init(frame: NSRect) { super.init(frame: frame); setAccessibilityLabel(L10n.text("画布导航，点击移动视野")) }
    convenience init() { self.init(frame: .zero) }
    required init?(coder: NSCoder) { nil }
    override func draw(_ dirtyRect: NSRect) {
        WorkspaceColors.panel.setFill(); NSBezierPath(roundedRect: bounds, xRadius: 8, yRadius: 8).fill()
        WorkspaceColors.border.setStroke(); NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 8, yRadius: 8).stroke()
        (L10n.text("导航") as NSString).draw(at: NSPoint(x: 8, y: 5), withAttributes: [.font: NSFont.systemFont(ofSize: 9), .foregroundColor: WorkspaceColors.muted])
        guard let artboard, !viewport.isEmpty else { return }
        let union = artboard.union(viewport)
        mapScale = min((bounds.width - 20) / max(1, union.width), (bounds.height - 29) / max(1, union.height))
        mapOrigin = CGPoint(x: (bounds.width - union.width * mapScale) / 2 - union.minX * mapScale, y: 23 - union.minY * mapScale)
        func map(_ rect: CGRect) -> CGRect { CGRect(x: mapOrigin.x + rect.minX * mapScale, y: mapOrigin.y + rect.minY * mapScale, width: rect.width * mapScale, height: rect.height * mapScale) }
        WorkspaceColors.secondary.withAlphaComponent(0.35).setFill(); map(artboard).fill()
        let visible = NSBezierPath(rect: map(viewport)); WorkspaceColors.accent.withAlphaComponent(0.15).setFill(); visible.fill()
        WorkspaceColors.accent.setStroke(); visible.lineWidth = 1; visible.stroke()
    }
    override func mouseDown(with event: NSEvent) { navigate(event) }
    override func mouseDragged(with event: NSEvent) { navigate(event) }
    private func navigate(_ event: NSEvent) {
        guard artboard != nil, mapScale > 0 else { return }
        let point = convert(event.locationInWindow, from: nil)
        onNavigate?(viewport.midX - (point.x - mapOrigin.x) / mapScale, viewport.midY - (point.y - mapOrigin.y) / mapScale)
    }
}
