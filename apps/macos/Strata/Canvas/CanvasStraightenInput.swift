import AppKit
import Combine

struct StraightenLine:Equatable{
    let start:NSPoint
    let end:NSPoint
    var valid:Bool{[start.x,start.y,end.x,end.y].allSatisfy{$0.isFinite&&abs($0)<=1e12} && hypot(end.x-start.x,end.y-start.y)>=0.000001}
    func correction(align:String)->Double?{
        guard valid,["horizontal","vertical"].contains(align)else{return nil}
        let target=align=="horizontal" ? 0.0:90.0
        var value=(target-atan2(Double(end.y-start.y),Double(end.x-start.x))*180/Double.pi+90).truncatingRemainder(dividingBy:180)
        if value<0{value+=180};value-=90
        guard abs(value)<=45 else{return nil};return abs(value)<1e-12 ? 0:value
    }
}

/// Only the guide is staged. The core request/token captures the original
/// document, revision and control epoch before the first pointer sample.
@MainActor
final class CanvasStraightenInput:ObservableObject{
    @Published private(set) var running=false
    @Published private(set) var error=""
    @Published private(set) var draft:StraightenLine?
    @Published private(set) var align="horizontal"
    private let engine:StrataEngine
    private var context:(document:String?,revision:UInt64,control:String)?
    private var token:String?
    private var enabled=false
    private var dragging=false
    private var autoCrop=true
    private var interpolation="bilinear"
    var onChanged:(()->Void)?
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    var onError:((Error)->Void)?
    var hasDraft:Bool{token != nil}
    var correction:Double?{draft?.correction(align:align)}
    var canConfirm:Bool{hasDraft && correction != nil && !running}
    init(engine:StrataEngine){self.engine=engine}
    func bind(_ snapshot:EditorSnapshot){
        let control=snapshot.control?.kind ?? "user"
        if context?.document != snapshot.documentId || context?.revision != snapshot.revision || context?.control != control{cancel()}
        context=(snapshot.documentId,snapshot.revision,control)
    }
    func configure(enabled:Bool,align:String="horizontal",autoCrop:Bool=true,interpolation:String="bilinear"){
        guard enabled else{self.enabled=false;cancel();return}
        guard !running else{return}
        self.enabled=true;self.align=align;self.autoCrop=autoCrop;self.interpolation=interpolation
        validateLine();onChanged?()
    }
    func down(_ point:NSPoint){
        guard enabled,!running,context?.control=="user",let context,let document=context.document,[point.x,point.y].allSatisfy({$0.isFinite&&abs($0)<=1e12})else{return}
        cancel()
        do{token=try engine.beginStraighten(documentId:document,revision:context.revision);dragging=true;publish(StraightenLine(start:point,end:point))}catch{record(error)}
    }
    func drag(_ point:NSPoint){
        guard enabled,!running,dragging,let draft else{return}
        publish(StraightenLine(start:draft.start,end:point))
    }
    func up(_ point:NSPoint){drag(point);dragging=false;validateLine()}
    private func publish(_ line:StraightenLine?){draft=line;error="";onChanged?()}
    private func validateLine(){
        guard let draft else{error="";return}
        if !draft.valid{error=L10n.text("请拖出两个有效且不同的参考点。")}
        else if correction == nil{error=L10n.text("校正超过 ±45°；请选择另一对齐方向。")}
        else{error=""}
    }
    @discardableResult
    func confirm()->Bool{
        guard !running,let token else{return false}
        guard let draft,correction != nil else{validateLine();return true}
        do{
            let encoded=try commandJSON(["reference":["type":"line","start":[Double(draft.start.x),Double(draft.start.y)],"end":[Double(draft.end.x),Double(draft.end.y)],"align":align],"crop":autoCrop,"interpolation":interpolation]),engine=engine
            dragging=false;running=true;onRunning?(true)
            Task{[weak self]in
                let result:Result<Void,Error>
                do{try await aiEngineWork{try engine.endStraighten(token:token,paramsJson:encoded,cancel:false)};result = .success(())}catch{result = .failure(error)}
                guard let self else{return};self.running=false
                if self.token==token{self.token=nil;self.publish(nil)}
                self.onRunning?(false)
                switch result{case .success:self.onCommitted?();case .failure(let error):self.record(error)}
            }
        }catch{record(error)}
        return true
    }
    func cancel(){if let token{try? engine.endStraighten(token:token,paramsJson:"{}",cancel:true)};token=nil;dragging=false;publish(nil)}
    func path()->CGPath?{
        guard let draft,[draft.start.x,draft.start.y,draft.end.x,draft.end.y].allSatisfy({$0.isFinite&&abs($0)<=1e12})else{return nil}
        let path=CGMutablePath();path.move(to:draft.start);path.addLine(to:draft.end);return path
    }
    private func record(_ error:Error){if let e=error as? StrataError,case .Cancelled=e{return};self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;onError?(error)}
}
