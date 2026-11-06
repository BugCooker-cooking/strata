import AppKit
import Combine

/// An owned crop draft draws only an overlay. Its trusted core request is
/// captured on mouse-down; applying never retargets a newer document or epoch.
@MainActor
final class CanvasCropInput:ObservableObject{
    @Published private(set) var running=false
    @Published private(set) var error=""
    @Published private(set) var draft:CGRect?
    private let engine:StrataEngine
    private var context:(document:String?,revision:UInt64,control:String)?
    private var token:String?
    private var active=false
    private var enabled=false
    private var start=NSPoint.zero
    private var current=NSPoint.zero
    private var ratio:[UInt32]?
    var onChanged:(()->Void)?
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    var onError:((Error)->Void)?
    var hasDraft:Bool{token != nil}
    init(engine:StrataEngine){self.engine=engine}
    func bind(_ snapshot:EditorSnapshot){
        let control=snapshot.control?.kind ?? "user"
        if context?.document != snapshot.documentId || context?.revision != snapshot.revision || context?.control != control{cancel()}
        context=(snapshot.documentId,snapshot.revision,control)
    }
    func configure(enabled:Bool,ratio:String){
        self.enabled=enabled
        if !enabled{cancel();return}
        guard !running else{return}
        let pair=ratio.split(separator:":").compactMap{UInt32($0)}
        self.ratio=pair.count==2&&pair.allSatisfy({$0>0&&$0<=100000}) ? pair:nil
        updateDraft()
    }
    func down(_ point:NSPoint){
        guard enabled,!running,context?.control=="user",let context,let document=context.document,point.x.isFinite,point.y.isFinite else{return}
        cancel();error=""
        do{token=try engine.beginCrop(documentId:document,revision:context.revision);active=true;start=point;current=point;updateDraft()}
        catch{record(error)}
    }
    func drag(_ point:NSPoint){guard active,!running,point.x.isFinite,point.y.isFinite else{return};current=point;updateDraft()}
    func up(_ point:NSPoint){drag(point)}
    private var rectangle:[String:Any]?{
        let x=min(start.x,current.x).rounded(.down),y=min(start.y,current.y).rounded(.down)
        let width=max(start.x,current.x).rounded(.up)-x,height=max(start.y,current.y).rounded(.up)-y
        guard abs(x)<=1e12,abs(y)<=1e12,let left=Int64(exactly:x),let top=Int64(exactly:y),let w=UInt32(exactly:width),let h=UInt32(exactly:height),w>0,h>0,w<=100000,h<=100000 else{return nil}
        return ["x":left,"y":top,"width":w,"height":h]
    }
    private func updateDraft(){
        guard token != nil,let rectangle,let x=rectangle["x"] as? Int64,let y=rectangle["y"] as? Int64,var w=rectangle["width"] as? UInt32,var h=rectangle["height"] as? UInt32 else{draft=nil;onChanged?();return}
        if let ratio{var a=ratio[0],b=ratio[1];while b != 0{(a,b)=(b,a%b)};let rw=ratio[0]/a,rh=ratio[1]/a,multiple=min(w/rw,h/rh);guard multiple>0 else{draft=nil;onChanged?();return};w=rw*multiple;h=rh*multiple}
        draft=CGRect(x:Double(x),y:Double(y),width:Double(w),height:Double(h));error="";onChanged?()
    }
    @discardableResult
    func confirm()->Bool{
        guard active,!running,let token else{return false}
        guard let rectangle,draft != nil else{error="请拖出有效的裁剪矩形。";return true}
        var params:[String:Any]=["rect":rectangle,"pixels":"preserve"];if let ratio{params["aspect_ratio"]=ratio}
        do{
            let encoded=try commandJSON(params),engine=engine
            active=false;running=true;onRunning?(true)
            Task{[weak self]in
                let result:Result<Void,Error>
                do{try await aiEngineWork{try engine.endCrop(token:token,paramsJson:encoded,cancel:false)};result = .success(())}catch{result = .failure(error)}
                guard let self else{return};self.running=false
                if self.token==token{self.token=nil;self.draft=nil;self.active=false;self.onChanged?()}
                self.onRunning?(false)
                switch result{case .success:self.onCommitted?();case .failure(let error):self.record(error)}
            }
        }catch{record(error)}
        return true
    }
    func cancel(){
        if let token{try? engine.endCrop(token:token,paramsJson:"{}",cancel:true)}
        token=nil;active=false;draft=nil;error="";onChanged?()
    }
    func path()->CGPath?{guard let draft else{return nil};return CGPath(rect:draft,transform:nil)}
    private func record(_ error:Error){if let e=error as? StrataError,case .Cancelled=e{return};self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;onError?(error)}
}
