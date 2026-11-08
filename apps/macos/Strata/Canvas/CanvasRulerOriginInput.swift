import AppKit
import Combine

/// Draft lines only; the owned release commits the shared view command once.
@MainActor
final class CanvasRulerOriginInput:ObservableObject{
    private let engine:StrataEngine
    private var snapshot:EditorSnapshot?
    private var token:String?
    private var generation=UUID()
    @Published private(set) var draft:NSPoint?
    @Published private(set) var running=false
    @Published private(set) var error=""
    var onBegan:(()->Void)?
    var onChanged:(()->Void)?
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    init(engine:StrataEngine){self.engine=engine}
    func bind(_ value:EditorSnapshot){
        if snapshot?.documentId != value.documentId || snapshot?.revision != value.revision || snapshot?.presentation?.version != value.presentation?.version || snapshot?.control?.kind != value.control?.kind{cancel()}
        snapshot=value
    }
    private func valid(_ point:NSPoint)->Bool{point.x.isFinite && point.y.isFinite && abs(point.x)<=1e12 && abs(point.y)<=1e12}
    @discardableResult func begin(at point:NSPoint)->Bool{
        guard !running,valid(point),let state=snapshot,let document=state.documentId,state.presentation?.rulersVisible != false else{return false}
        cancel();onBegan?();error=""
        do{token=try engine.beginRulerOrigin(documentId:document,revision:state.revision,presentationVersion:state.presentation?.version ?? 0);draft=point;onChanged?();return true}
        catch{self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;return false}
    }
    func move(to point:NSPoint){guard token != nil,!running,valid(point)else{return};draft=point;onChanged?()}
    func cancel(){generation=UUID();if let token{try? engine.endRulerOrigin(token:token,x:0,y:0,cancel:true)};token=nil;draft=nil;onChanged?()}
    @discardableResult func finish(at point:NSPoint?)->Bool{
        guard !running,let token else{return false}
        guard let point,valid(point)else{cancel();return false}
        draft=nil;onChanged?();running=true;onRunning?(true)
        let engine=engine,current=generation
        Task{[weak self]in
            let result:Result<Void,Error>
            do{result = .success(try await aiEngineWork{try engine.endRulerOrigin(token:token,x:point.x,y:point.y,cancel:false)})}catch{result = .failure(error)}
            guard let self else{return};self.running=false;self.onRunning?(false)
            switch result{
            case .success:self.cancel();self.onCommitted?()
            case .failure(let error):if current==self.generation{self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;self.cancel()}
            }
        };return true
    }
    func reset(){if begin(at:.zero){_ = finish(at:.zero)}}
}
