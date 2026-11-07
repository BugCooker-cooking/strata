import AppKit
import Combine

private struct CanvasTransformFailure:LocalizedError,Sendable{let message:String;var errorDescription:String?{message}}

enum CanvasTransformMode:String,CaseIterable{
    case scale,rotate,skew,distort,perspective
    var title:String{switch self{case .scale:"缩放";case .rotate:"旋转";case .skew:"斜切";case .distort:"扭曲";case .perspective:"透视"}}
}
enum TransformHandle:CaseIterable{
    case topLeft,top,topRight,right,bottomRight,bottom,bottomLeft,left
    var axes:(Int,Int){switch self{case .topLeft:(-1,-1);case .top:(0,-1);case .topRight:(1,-1);case .right:(1,0);case .bottomRight:(1,1);case .bottom:(0,1);case .bottomLeft:(-1,1);case .left:(-1,0)}}
    var corner:Int?{switch self{case .topLeft:0;case .topRight:1;case .bottomRight:2;case .bottomLeft:3;default:nil}}
    var edge:(Int,Int){switch self{case .topLeft,.top,.topRight:(0,1);case .right:(1,2);case .bottomRight,.bottom,.bottomLeft:(3,2);case .left:(0,3)}}
    var opposite:Self{switch self{case .topLeft:.bottomRight;case .top:.bottom;case .topRight:.bottomLeft;case .right:.left;case .bottomRight:.topLeft;case .bottom:.top;case .bottomLeft:.topRight;case .left:.right}}
    func position(in quad:TransformQuad)->NSPoint{if let corner{return quad.points[corner]};let(a,b)=edge;return TransformQuad.multiply(TransformQuad.add(quad.points[a],quad.points[b]),0.5)}
}

/// Geometry only: pixel sampling is exclusively the core transform.apply.
struct TransformQuad:Equatable{
    var points:[NSPoint]
    init(_ points:[NSPoint]){self.points=points}
    init(rect:CGRect){points=[NSPoint(x:rect.minX,y:rect.minY),NSPoint(x:rect.maxX,y:rect.minY),NSPoint(x:rect.maxX,y:rect.maxY),NSPoint(x:rect.minX,y:rect.maxY)]}
    static func add(_ a:NSPoint,_ b:NSPoint)->NSPoint{NSPoint(x:a.x+b.x,y:a.y+b.y)}
    static func subtract(_ a:NSPoint,_ b:NSPoint)->NSPoint{NSPoint(x:a.x-b.x,y:a.y-b.y)}
    static func multiply(_ a:NSPoint,_ s:CGFloat)->NSPoint{NSPoint(x:a.x*s,y:a.y*s)}
    static func dot(_ a:NSPoint,_ b:NSPoint)->CGFloat{a.x*b.x+a.y*b.y}
    static func cross(_ a:NSPoint,_ b:NSPoint)->CGFloat{a.x*b.y-a.y*b.x}
    var center:NSPoint{points.reduce(.zero){Self.add($0,Self.multiply($1,0.25))}}
    var valid:Bool{
        guard points.count==4,points.allSatisfy({[$0.x,$0.y].allSatisfy{$0.isFinite&&abs($0)<=1e12}})else{return false}
        let edges=(0..<4).map{Self.subtract(points[($0+1)%4],points[$0])}
        let scale=edges.map{hypot($0.x,$0.y)}.max() ?? 0
        guard scale>0.000001 else{return false}
        let crosses=(0..<4).map{Self.cross(Self.multiply(edges[$0],1/scale),Self.multiply(edges[($0+1)%4],1/scale))}
        return crosses.allSatisfy{$0>1e-10} || crosses.allSatisfy{$0 < -1e-10}
    }
    func contains(_ p:NSPoint)->Bool{
        guard valid else{return false};let crosses=(0..<4).map{Self.cross(Self.subtract(points[($0+1)%4],points[$0]),Self.subtract(p,points[$0]))}
        return crosses.allSatisfy{$0>=0} || crosses.allSatisfy{$0<=0}
    }
    func moved(_ delta:NSPoint)->Self{Self(points.map{Self.add($0,delta)})}
    func rotated(_ angle:CGFloat,around pivot:NSPoint)->Self{
        let quarter=angle/(CGFloat.pi/2),rounded=quarter.rounded()
        let c:CGFloat,s:CGFloat
        if abs(quarter-rounded)<1e-12{let n=(Int(rounded.truncatingRemainder(dividingBy:4))+4)%4;c=[1,0,-1,0][n];s=[0,1,0,-1][n]}
        else{c=cos(angle);s=sin(angle)}
        return Self(points.map{let p=Self.subtract($0,pivot);return Self.add(pivot,NSPoint(x:c*p.x-s*p.y,y:s*p.x+c*p.y))})
    }
    func rotationHandle(distance:CGFloat)->NSPoint{
        let top=TransformHandle.top.position(in:self),v=Self.subtract(top,center),length=hypot(v.x,v.y)
        return Self.add(top,length>1e-12 ? Self.multiply(v,distance/length):NSPoint(x:0,y:-distance))
    }
    func resized(handle:TransformHandle,to point:NSPoint,uniform:Bool,centered:Bool)->Self{
        let(a,b)=handle.axes,pivot=centered ? center:handle.opposite.position(in:self)
        let u=Self.multiply(Self.add(Self.subtract(points[1],points[0]),Self.subtract(points[2],points[3])),0.5)
        let v=Self.multiply(Self.add(Self.subtract(points[3],points[0]),Self.subtract(points[2],points[1])),0.5)
        let determinant=Self.cross(u,v)
        guard abs(determinant)>1e-12 else{return self}
        func local(_ p:NSPoint)->NSPoint{let d=Self.subtract(p,pivot);return NSPoint(x:Self.cross(d,v)/determinant,y:Self.cross(u,d)/determinant)}
        let old=local(handle.position(in:self)),next=local(point)
        var sx=a==0 ? 1:next.x/old.x,sy=b==0 ? 1:next.y/old.y
        if uniform{let factor=a==0 ? sy:b==0 ? sx:abs(sx-1)>=abs(sy-1) ? sx:sy;sx=factor;sy=factor}
        return Self(points.map{let p=local($0);return Self.add(pivot,Self.add(Self.multiply(u,p.x*sx),Self.multiply(v,p.y*sy)))})
    }
    func skewed(handle:TransformHandle,delta:NSPoint)->Self{
        let(a,b)=handle.edge,edge=Self.subtract(points[b],points[a]),length=Self.dot(edge,edge)
        guard length>1e-12 else{return self};let movement=Self.multiply(edge,Self.dot(delta,edge)/length)
        var result=self;result.points[a]=Self.add(points[a],movement);result.points[b]=Self.add(points[b],movement);return result
    }
    func distorted(handle:TransformHandle,delta:NSPoint)->Self{
        var result=self
        if let corner=handle.corner{result.points[corner]=Self.add(points[corner],delta)}
        else{let(a,b)=handle.edge;result.points[a]=Self.add(points[a],delta);result.points[b]=Self.add(points[b],delta)}
        return result
    }
    func perspective(handle:TransformHandle,delta:NSPoint)->Self{
        guard let i=handle.corner else{return self}
        let u=Self.multiply(Self.add(Self.subtract(points[1],points[0]),Self.subtract(points[2],points[3])),0.5)
        let v=Self.multiply(Self.add(Self.subtract(points[3],points[0]),Self.subtract(points[2],points[1])),0.5)
        let ul=hypot(u.x,u.y),vl=hypot(v.x,v.y);guard ul>1e-12,vl>1e-12 else{return self}
        let ux=Self.multiply(u,1/ul),vy=Self.multiply(v,1/vl),dx=Self.dot(delta,ux),dy=Self.dot(delta,vy)
        let j:Int,movement:NSPoint
        if abs(dx)>=abs(dy){j=[1,0,3,2][i];movement=Self.multiply(ux,dx)}
        else{j=[3,2,1,0][i];movement=Self.multiply(vy,dy)}
        var result=self;result.points[i]=Self.add(points[i],movement);result.points[j]=Self.subtract(points[j],movement);return result
    }
    var json:[[Double]]{points.map{[Double($0.x),Double($0.y)]}}
}

/// Owned original-context draft. Pointer samples only change the outline; Enter
/// applies the same strict corner geometry command as native numeric UI and AI.
@MainActor
final class CanvasTransformInput:ObservableObject{
    @Published private(set) var running=false
    @Published private(set) var error=""
    @Published private(set) var draft:TransformQuad?
    @Published private(set) var source:TransformQuad?
    private let engine:StrataEngine
    private var context:(document:String?,revision:UInt64,layer:String?,control:String)?
    private var token:String?
    private var generation=UUID()
    private var enabled=false
    private var scope="whole_layer"
    private var interpolation="bilinear"
    private var mode=CanvasTransformMode.scale
    private enum Drag{case move(NSPoint,TransformQuad),resize(TransformHandle,NSPoint,TransformQuad),rotate(NSPoint,TransformQuad),skew(TransformHandle,NSPoint,TransformQuad),distort(TransformHandle,NSPoint,TransformQuad),perspective(TransformHandle,NSPoint,TransformQuad)}
    private var gesture:Drag?
    private var lastValid:TransformQuad?
    var onChanged:(()->Void)?
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    var onError:((Error)->Void)?
    var hasDraft:Bool{token != nil || draft != nil}
    var handles:[TransformHandle]{TransformHandle.allCases.filter{mode != .perspective || $0.corner != nil}}
    var canConfirm:Bool{token != nil && draft?.valid==true && source?.valid==true && !running}
    init(engine:StrataEngine){self.engine=engine}
    func bind(_ snapshot:EditorSnapshot){
        let control=snapshot.control?.kind ?? "user"
        if context?.document != snapshot.documentId || context?.revision != snapshot.revision || context?.layer != snapshot.activeLayer || context?.control != control{cancel()}
        context=(snapshot.documentId,snapshot.revision,snapshot.activeLayer,control)
    }
    func configure(enabled:Bool,mode:String="scale",scope:String="whole_layer",interpolation:String="bilinear"){
        if !enabled{self.enabled=false;cancel();return}
        guard !running else{return}
        if self.scope != scope{cancel()}
        let next=CanvasTransformMode(rawValue:mode) ?? .scale;if self.mode != next{gesture=nil};self.enabled=true;self.mode=next;self.scope=scope;self.interpolation=interpolation;onChanged?()
    }
    func start(){
        guard enabled,!running,let context,context.control=="user",let document=context.document,let layer=context.layer else{return}
        cancel();error="";let current=generation,engine=engine,scope=scope;running=true;onRunning?(true)
        Task{[weak self]in
            guard let self else{return}
            defer{self.running=false;self.onRunning?(false);self.onChanged?()}
            var created:String?
            do{
                let owner=try await aiEngineWork{try engine.beginCanvasTransform(documentId:document,revision:context.revision,layerId:layer,scope:scope)};created=owner
                guard self.generation==current else{try? engine.endCanvasTransform(token:owner,paramsJson:"{}",cancel:true);return}
                self.token=owner
                let json=try await aiEngineWork{try engine.canvasTransformBounds(token:owner)}
                guard self.generation==current,self.token==owner else{return}
                let value=try AIJSON.decode(json)["bounds"]
                guard let x=value["x"].number,let y=value["y"].number,let w=value["width"].number,let h=value["height"].number,w>0,h>0 else{throw CanvasTransformFailure(message:"没有可变换的像素或选区。")}
                let quad=TransformQuad(rect:CGRect(x:x,y:y,width:w,height:h));guard quad.valid else{throw CanvasTransformFailure(message:"变换边界超出有效范围。")}
                self.source=quad;self.draft=quad;self.lastValid=quad
            }catch{
                if let created{try? engine.endCanvasTransform(token:created,paramsJson:"{}",cancel:true)}
                if self.generation==current{self.token=nil;self.record(error)}
            }
        }
    }
    func down(_ point:NSPoint,hitTolerance:CGFloat,modifiers:NSEvent.ModifierFlags=[]){
        guard enabled,!running,let draft,draft.valid,valid(point)else{return}
        let tolerance=max(hitTolerance,0.000001)
        if distance(point,draft.rotationHandle(distance:tolerance*3))<=tolerance{gesture = .rotate(point,draft);return}
        if let handle=handles.min(by:{distance(point,$0.position(in:draft))<distance(point,$1.position(in:draft))}),distance(point,handle.position(in:draft))<=tolerance{
            switch mode{case .scale:gesture = .resize(handle,point,draft);case .rotate:gesture = .rotate(point,draft);case .skew:gesture = .skew(handle,point,draft);case .distort:gesture = .distort(handle,point,draft);case .perspective:gesture = .perspective(handle,point,draft)};return
        }
        if mode == .rotate{gesture = .rotate(point,draft)}else if draft.contains(point){gesture = .move(point,draft)}else{gesture=nil}
    }
    func drag(_ point:NSPoint,modifiers:NSEvent.ModifierFlags=[]){
        guard !running,valid(point),let gesture else{return}
        func delta(_ start:NSPoint)->NSPoint{let d=TransformQuad.subtract(point,start);return modifiers.contains(.shift) ? abs(d.x)>=abs(d.y) ? NSPoint(x:d.x,y:0):NSPoint(x:0,y:d.y):d}
        let next:TransformQuad
        switch gesture{
        case .move(let start,let base):next=base.moved(delta(start))
        case .resize(let handle,let start,let base):next=base.resized(handle:handle,to:TransformQuad.add(handle.position(in:base),TransformQuad.subtract(point,start)),uniform:modifiers.contains(.shift),centered:modifiers.contains(.option))
        case .rotate(let start,let base):
            let a=TransformQuad.subtract(start,base.center),b=TransformQuad.subtract(point,base.center)
            guard hypot(a.x,a.y)>1e-6,hypot(b.x,b.y)>1e-6 else{return}
            var angle=atan2(b.y,b.x)-atan2(a.y,a.x);if modifiers.contains(.shift){angle=(angle/(CGFloat.pi/12)).rounded()*(CGFloat.pi/12)};next=base.rotated(angle,around:base.center)
        case .skew(let handle,let start,let base):next=base.skewed(handle:handle,delta:TransformQuad.subtract(point,start))
        case .distort(let handle,let start,let base):next=base.distorted(handle:handle,delta:delta(start))
        case .perspective(let handle,let start,let base):next=base.perspective(handle:handle,delta:TransformQuad.subtract(point,start))
        }
        draft=next;if next.valid{lastValid=next};error=next.valid ? "":"边框不能折叠、交叉或超出有效坐标。";onChanged?()
    }
    func up(_ point:NSPoint,modifiers:NSEvent.ModifierFlags=[]){drag(point,modifiers:modifiers);gesture=nil;if draft?.valid==false{draft=lastValid;error="无效边框已恢复到上一个有效位置。";onChanged?()}}
    @discardableResult func nudge(x:CGFloat,y:CGFloat)->Bool{
        guard !running,let draft else{return false};self.draft=draft.moved(NSPoint(x:x,y:y));if self.draft?.valid==true{lastValid=self.draft};error=self.draft?.valid==true ? "":"边框超出有效坐标。";onChanged?();return true
    }
    var parameters:[String:Any]?{
        guard canConfirm,let source,let draft,let layer=context?.layer else{return nil}
        return ["layer":["id":layer],"scope":scope,"geometry":["type":"corners","source":source.json,"destination":draft.json],"interpolation":interpolation]
    }
    @discardableResult func confirm()->Bool{
        guard !running,let token else{return false}
        guard let parameters else{error="请调整为有效的变换边框。";return true}
        do{
            let json=try commandJSON(parameters),engine=engine,current=generation;gesture=nil;running=true;onRunning?(true)
            Task{[weak self]in
                let result:Result<Void,Error>
                do{try await aiEngineWork{try engine.endCanvasTransform(token:token,paramsJson:json,cancel:false)};result = .success(())}catch{result = .failure(error)}
                guard let self else{return};self.running=false
                if self.generation==current{self.token=nil;self.source=nil;self.draft=nil}
                self.onRunning?(false);self.onChanged?()
                switch result{case .success:self.onCommitted?();case .failure(let error):if self.generation==current{self.record(error)}}
            }
        }catch{record(error)}
        return true
    }
    func cancel(){generation=UUID();if let token{try? engine.endCanvasTransform(token:token,paramsJson:"{}",cancel:true)};token=nil;gesture=nil;source=nil;draft=nil;lastValid=nil;error="";onChanged?()}
    func path()->CGPath?{guard let draft,draft.points.count==4,draft.points.allSatisfy({valid($0)})else{return nil};let path=CGMutablePath();path.addLines(between:draft.points);path.closeSubpath();return path}
    private func valid(_ p:NSPoint)->Bool{[p.x,p.y].allSatisfy{$0.isFinite&&abs($0)<=1e12}}
    private func distance(_ a:NSPoint,_ b:NSPoint)->CGFloat{hypot(a.x-b.x,a.y-b.y)}
    private func record(_ error:Error){if let e=error as? StrataError,case .Cancelled=e{return};self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription;onError?(error)}
}
