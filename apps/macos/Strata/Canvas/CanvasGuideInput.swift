import AppKit
import Combine

/// Ruler/line drags stage overlay coordinates only; one owned shared command at up.
@MainActor
final class CanvasGuideInput:ObservableObject{
    @Published private(set) var draft:(axis:String,position:Double)?
    @Published private(set) var running=false
    @Published private(set) var error=""
    private var snapshot:EditorSnapshot?
    private var token:String?
    private var target:String?
    private var generation=UUID()
    private let engine:StrataEngine
    var onChanged:(()->Void)?
    var onBegan:(()->Void)?
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    init(engine:StrataEngine){self.engine=engine}
    var canEdit:Bool{!running && snapshot?.documentId != nil && (snapshot?.control?.kind ?? "user")=="user" && snapshot?.presentation?.guidesLocked != true}
    func bind(_ value:EditorSnapshot){
        if snapshot?.documentId != value.documentId || snapshot?.revision != value.revision || snapshot?.presentation?.version != value.presentation?.version || snapshot?.control?.kind != value.control?.kind{cancel()}
        snapshot=value
    }
    func begin(axis:String,position:Double,guide:String?=nil){
        guard canEdit,["horizontal","vertical"].contains(axis),position.isFinite,abs(position)<=1e12,let state=snapshot,let document=state.documentId else{return}
        cancel();onBegan?();error=""
        do{token=try engine.beginGuide(documentId:document,revision:state.revision,presentationVersion:state.presentation?.version ?? 0,guideId:guide);target=guide;draft=(axis,position);onChanged?()}
        catch{self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription}
    }
    func move(position:Double){guard !running,let draft,position.isFinite,abs(position)<=1e12 else{return};self.draft=(draft.axis,position);onChanged?()}
    func cancel(){generation=UUID();if let token{try? engine.endGuide(token:token,paramsJson:"{}",cancel:true)};token=nil;draft=nil;target=nil;onChanged?()}
    @discardableResult func finish()->Bool{
        guard canEdit,let token,let draft else{return false}
        let parameters:[String:Any]
        if let target{parameters=["guide":["id":target],"position":draft.position]}
        else{parameters=["name":draft.axis=="vertical" ? "垂直参考线":"水平参考线","axis":draft.axis,"position":draft.position]}
        do{
            let json=try commandJSON(parameters),engine=engine,current=generation;running=true;onRunning?(true)
            Task{[weak self]in
                let result:Result<Void,Error>
                do{result = .success(try await aiEngineWork{try engine.endGuide(token:token,paramsJson:json,cancel:false)})}catch{result = .failure(error)}
                guard let self else{return};self.running=false;self.onRunning?(false)
                switch result{case .success:self.cancel();self.onCommitted?();case .failure(let error):if current==self.generation{self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;self.cancel()}}
            };return true
        }catch{self.error=error.localizedDescription;return false}
    }
}
