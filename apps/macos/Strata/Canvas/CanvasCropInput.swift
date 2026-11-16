import AppKit
import Combine

enum CropHandle:CaseIterable{
    case topLeft,top,topRight,left,right,bottomLeft,bottom,bottomRight
    var axes:(Int,Int){switch self{case .topLeft:(-1,-1);case .top:(0,-1);case .topRight:(1,-1);case .left:(-1,0);case .right:(1,0);case .bottomLeft:(-1,1);case .bottom:(0,1);case .bottomRight:(1,1)}}
    func position(in rect:CGRect)->NSPoint{let(x,y)=axes;return NSPoint(x:x<0 ? rect.minX:x>0 ? rect.maxX:rect.midX,y:y<0 ? rect.minY:y>0 ? rect.maxY:rect.midY)}
}

/// An owned crop draft draws only an overlay. Its trusted request is captured
/// on first input; moving/resizing the draft never retargets a newer revision.
@MainActor
final class CanvasCropInput:ObservableObject{
    @Published private(set) var running=false
    @Published private(set) var error=""
    @Published private(set) var draft:CGRect?
    private let engine:StrataEngine
    private var context:(document:String?,revision:UInt64,control:String)?
    private var token:String?
    private var enabled=false
    private var ratio:[UInt32]?
    private var deletesPixels=false
    private enum Drag{case create(NSPoint),move(NSPoint,CGRect),resize(CropHandle,CGRect)}
    private var gesture:Drag?
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
    func configure(enabled:Bool,ratio:String,deletePixels:Bool=false){
        self.enabled=enabled
        if !enabled{cancel();return}
        guard !running else{return}
        deletesPixels=deletePixels
        let pair=ratio.split(separator:":").compactMap{UInt32($0)}
        let next=pair.count==2&&pair.allSatisfy({$0>0&&$0<=100000}) ? pair:nil
        guard next != self.ratio else{return}
        self.ratio=next
        if let draft{publish(fit(draft))}
    }
    func down(_ point:NSPoint,hitTolerance:CGFloat=0){
        guard enabled,!running,context?.control=="user",let context,let document=context.document,point.x.isFinite,point.y.isFinite else{return}
        if token != nil,let draft{
            if hitTolerance>0,let handle=CropHandle.allCases.min(by:{distance(point,$0.position(in:draft))<distance(point,$1.position(in:draft))}),distance(point,handle.position(in:draft))<=hitTolerance{
                gesture = .resize(handle,draft);return
            }
            if draft.contains(point){gesture = .move(point,draft);return}
        }
        cancel();error=""
        do{token=try engine.beginCrop(documentId:document,revision:context.revision);gesture = .create(point);publish(nil)}catch{record(error)}
    }
    func drag(_ point:NSPoint){
        guard !running,point.x.isFinite,point.y.isFinite,let gesture else{return}
        switch gesture{
        case .create(let start):publish(fit(box(start,point)))
        case .move(let start,let rect):publish(valid(rect.offsetBy(dx:(point.x-start.x).rounded(),dy:(point.y-start.y).rounded())))
        case .resize(let handle,let rect):publish(resize(rect,handle:handle,to:point))
        }
    }
    func up(_ point:NSPoint){drag(point);gesture=nil}
    private func distance(_ a:NSPoint,_ b:NSPoint)->CGFloat{hypot(a.x-b.x,a.y-b.y)}
    private func box(_ a:NSPoint,_ b:NSPoint)->CGRect{
        let x=min(a.x,b.x).rounded(.down),y=min(a.y,b.y).rounded(.down)
        return CGRect(x:x,y:y,width:max(a.x,b.x).rounded(.up)-x,height:max(a.y,b.y).rounded(.up)-y)
    }
    private var units:(CGFloat,CGFloat)?{
        guard let ratio else{return nil};var a=ratio[0],b=ratio[1];while b != 0{(a,b)=(b,a%b)};return(CGFloat(ratio[0]/a),CGFloat(ratio[1]/a))
    }
    private func valid(_ rect:CGRect)->CGRect?{
        guard [rect.minX,rect.minY,rect.maxX,rect.maxY].allSatisfy({$0.isFinite&&abs($0)<=1e12&&$0.rounded()==$0}),rect.width>=1,rect.height>=1,rect.width<=100000,rect.height<=100000 else{return nil};return rect
    }
    private func fit(_ rect:CGRect)->CGRect?{
        guard let (w,h)=units else{return valid(rect)};let n=min(floor(rect.width/w),floor(rect.height/h));guard n>=1 else{return nil}
        return valid(CGRect(x:rect.minX,y:rect.minY,width:n*w,height:n*h))
    }
    private func resize(_ rect:CGRect,handle:CropHandle,to point:NSPoint)->CGRect?{
        let (ax,ay)=handle.axes
        let fixed=NSPoint(x:ax<0 ? rect.maxX:rect.minX,y:ay<0 ? rect.maxY:rect.minY)
        if ax != 0 && ay != 0{
            let bounds=box(fixed,point);guard var fitted=fit(bounds)else{return nil}
            fitted.origin.x=point.x<fixed.x ? fixed.x-fitted.width:fixed.x
            fitted.origin.y=point.y<fixed.y ? fixed.y-fitted.height:fixed.y
            return valid(fitted)
        }
        if ax != 0{
            let width=abs(point.x-fixed.x).rounded(.up)
            let size:CGSize
            if let(w,h)=units{let n=floor(width/w);size=CGSize(width:n*w,height:n*h)}else{size=CGSize(width:width,height:rect.height)}
            return valid(CGRect(x:point.x<fixed.x ? fixed.x-size.width:fixed.x,y:(rect.midY-size.height/2).rounded(.down),width:size.width,height:size.height))
        }
        let height=abs(point.y-fixed.y).rounded(.up)
        let size:CGSize
        if let(w,h)=units{let n=floor(height/h);size=CGSize(width:n*w,height:n*h)}else{size=CGSize(width:rect.width,height:height)}
        return valid(CGRect(x:(rect.midX-size.width/2).rounded(.down),y:point.y<fixed.y ? fixed.y-size.height:fixed.y,width:size.width,height:size.height))
    }
    private func publish(_ rect:CGRect?){draft=rect;if rect != nil{error=""};onChanged?()}
    @discardableResult
    func confirm()->Bool{
        guard !running,let token else{return false}
        guard let draft=valid(draft ?? .zero),let x=Int64(exactly:draft.minX),let y=Int64(exactly:draft.minY),let w=UInt32(exactly:draft.width),let h=UInt32(exactly:draft.height)else{error=L10n.text("请拖出有效的裁剪矩形。");return true}
        var params:[String:Any]=["rect":["x":x,"y":y,"width":w,"height":h],"pixels":deletesPixels ? "delete":"preserve"];if let ratio{params["aspect_ratio"]=ratio}
        do{
            let encoded=try commandJSON(params),engine=engine;gesture=nil;running=true;onRunning?(true)
            Task{[weak self]in
                let result:Result<Void,Error>
                do{try await aiEngineWork{try engine.endCrop(token:token,paramsJson:encoded,cancel:false)};result = .success(())}catch{result = .failure(error)}
                guard let self else{return};self.running=false
                if self.token==token{self.token=nil;self.publish(nil)}
                self.onRunning?(false)
                switch result{case .success:self.onCommitted?();case .failure(let error):self.record(error)}
            }
        }catch{record(error)}
        return true
    }
    func cancel(){if let token{try? engine.endCrop(token:token,paramsJson:"{}",cancel:true)};token=nil;gesture=nil;error="";publish(nil)}
    func path()->CGPath?{guard let draft else{return nil};return CGPath(rect:draft,transform:nil)}
    private func record(_ error:Error){if let e=error as? StrataError,case .Cancelled=e{return};self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;onError?(error)}
}
