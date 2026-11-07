import AppKit
import Combine

/// Reuses the one bounded shared pixel-preview slot and actual snap command.
@MainActor
final class CanvasSnapInput:ObservableObject{
    private let preview:TonalPreviewModel
    private var layer:String?
    private var origin=NSPoint.zero
    private var start=NSPoint.zero
    private var tolerance=0.0
    private var artboard=true
    private var parameters:String?
    private var snapshot:EditorSnapshot?
    private var moved=false
    @Published private(set) var error=""
    var active:Bool{layer != nil}
    var busy:Bool{preview.busy || preview.applying}
    init(engine:StrataEngine){preview=TonalPreviewModel(engine:engine);preview.configureFilter("layer.snap_position")}
    var onFrame:(()->Void)?{didSet{preview.onFrame={[weak self]in guard let self else{return};self.error=self.preview.error;self.onFrame?()}}}
    var onBegan:(()->Void)?
    var onApplying:((Bool)->Void)?{didSet{preview.onApplying=onApplying}}
    func bind(_ value:EditorSnapshot){
        if snapshot?.documentId != value.documentId || snapshot?.revision != value.revision || snapshot?.activeLayer != value.activeLayer || snapshot?.presentation?.version != value.presentation?.version || snapshot?.control?.kind != value.control?.kind{cancel()}
        snapshot=value;preview.bind(value)
    }
    func begin(at point:NSPoint,canvasPixelsPerPoint:Double)->Bool{
        guard !active,canvasPixelsPerPoint.isFinite,canvasPixelsPerPoint>0,let state=snapshot,let selected=state.layers.first(where:{$0.id==state.activeLayer}),let presentation=state.presentation,presentation.snapEnabled,(state.control?.kind ?? "user")=="user",selected.visible else{return false}
        onBegan?();error="";moved=false;layer=selected.id;origin=NSPoint(x:Double(selected.x),y:Double(selected.y));start=point;tolerance=min(1e6,presentation.snapTolerance*canvasPixelsPerPoint);artboard=presentation.snapArtboard
        preview.configureFilter("layer.snap_position");preview.bind(state);parameters=encode(point);return true
    }
    func move(to point:NSPoint){guard active else{return};moved=moved || point != start;parameters=encode(point);if preview.enabled{preview.update(parameters)}else{preview.setEnabled(true,settings:parameters)}}
    func finish(at point:NSPoint){guard active else{return};if !moved && point==start{cancel();return};move(to:point);if let parameters{preview.apply(parameters)};layer=nil;parameters=nil}
    func cancel(){layer=nil;parameters=nil;preview.cancel()}
    private func encode(_ point:NSPoint)->String?{
        let x=(origin.x+point.x-start.x).rounded(),y=(origin.y+point.y-start.y).rounded()
        guard x.isFinite,y.isFinite,abs(x)<=1e12,abs(y)<=1e12,let ix=Int64(exactly:x),let iy=Int64(exactly:y)else{return nil}
        // Preview ownership inserts its original layer id, so host does not
        // retarget through a later current-layer selection.
        return try? commandJSON(["x":ix,"y":iy,"tolerance":tolerance,"artboard":artboard])
    }
}
