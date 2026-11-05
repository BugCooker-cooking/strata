import AppKit

struct CanvasPaintConfiguration:Equatable{
    var command="paint.gradient"
    var layer:String?
    var revision:UInt64=0
    var foreground=[0.0,0,0,1],background=[1.0,1,1,1]
    var opacity=1.0,tolerance=32,contiguous=true,sample="merged",radius=0,shape="linear",reverse=false
}
@MainActor
final class CanvasPaintInput{
    private let engine:StrataEngine
    private var settings=CanvasPaintConfiguration()
    private var active=false
    private var start=NSPoint.zero,current=NSPoint.zero
    private(set) var running=false
    var onChanged:(()->Void)?
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    var onSample:(([Double],Bool,String)->Void)?
    var onError:((Error)->Void)?
    init(engine:StrataEngine){self.engine=engine}
    func configure(_ value:CanvasPaintConfiguration){if settings != value{cancel()};settings=value}
    func down(_ point:NSPoint,background:Bool){
        guard !running else{return};start=point;current=point
        if settings.command=="paint.sample_color"{sample(point,background:background);return}
        guard let layer=settings.layer else{return}
        do{try engine.beginPaint(layerId:layer,command:settings.command);active=true;onChanged?()
            if settings.command=="paint.bucket"{submit(bucket(point))}
        }catch{onError?(error)}
    }
    func drag(_ point:NSPoint){guard active,!running else{return};current=point;onChanged?()}
    func up(_ point:NSPoint){
        guard active,!running,settings.command=="paint.gradient" else{return};current=point
        guard hypot(current.x-start.x,current.y-start.y)>0.001 else{cancel();return}
        submit(["start":[Double(start.x),Double(start.y)],"end":[Double(current.x),Double(current.y)],"shape":settings.shape,"reverse":settings.reverse,"opacity":settings.opacity,
            "stops":[["at":0,"color":settings.foreground] as [String:Any],["at":1,"color":settings.background] as [String:Any]]])
    }
    func cancel(){
        if running{try? engine.cancelPendingWork()}else if active{try? engine.endPaint(paramsJson:"{}",cancel:true)}
        let changed=active;active=false;if changed{onChanged?()}
    }
    func path()->CGPath?{
        guard active else{return nil};let path=CGMutablePath();path.move(to:start);path.addLine(to:current);return path
    }
    private func bucket(_ point:NSPoint)->[String:Any]{["x":Int64(point.x.rounded(.down)),"y":Int64(point.y.rounded(.down)),"color":settings.foreground,"opacity":settings.opacity,"tolerance":settings.tolerance,"contiguous":settings.contiguous,"sample":settings.sample]}
    private func submit(_ params:[String:Any]){
        do{let encoded=try commandJSON(params),engine=engine;active=false;running=true;onChanged?();onRunning?(true)
            Task{[weak self] in
                let result:Result<Void,Error>
                do{try await aiEngineWork{try engine.endPaint(paramsJson:encoded,cancel:false)};result = .success(())}catch{result = .failure(error)}
                guard let self else{return};self.running=false;self.onRunning?(false)
                switch result{case .success:self.onCommitted?();case .failure(let error):self.onError?(error)}
            }
        }catch{cancel();onError?(error)}
    }
    private func sample(_ point:NSPoint,background:Bool){
        do{let parameters=try commandJSON(["x":Int64(point.x.rounded(.down)),"y":Int64(point.y.rounded(.down)),"radius":settings.radius,"sample":settings.sample]),revision=settings.revision,engine=engine
            running=true;onRunning?(true)
            Task{[weak self] in
                do{let result=try await aiEngineWork{try engine.sampleColor(paramsJson:parameters,revision:revision)}
                    let value=try AIJSON.decode(result),color=value["color"].array.compactMap(\.number)
                    guard let self else{return};self.running=false;self.onRunning?(false)
                    if color.count==4{self.onSample?(color,background,value["profile"].string ?? "RGB")}
                }catch{guard let self else{return};self.running=false;self.onRunning?(false);self.onError?(error)}
            }
        }catch{onError?(error)}
    }
}
