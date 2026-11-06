import Foundation
import Combine

struct PaintColorRepresentations:Decodable,Sendable{
    let rgba:[Double]
    let rgb8:[UInt8]
    let rgb16:[UInt16]
    let hsb:[Double]
    let hex:String
}
struct PaintColorDescription:Decodable,Sendable{
    let foreground:PaintColorRepresentations
    let background:PaintColorRepresentations
    let version:UInt64
}

@MainActor
final class ColorPanelModel:ObservableObject{
    @Published private(set) var state:PaintColorDescription?
    @Published private(set) var error=""
    private let engine:StrataEngine
    private var ticket:UInt64=0
    private var loadingVersion:UInt64?
    private var observedVersion:UInt64=0
    init(engine:StrataEngine){self.engine=engine}
    func bind(_ snapshot:EditorSnapshot){
        let version=snapshot.paintColors?.version ?? 0;observedVersion=version
        guard state?.version != version,loadingVersion != version else{return}
        ticket &+= 1;let request=ticket,engine=engine;loadingVersion=version
        Task{[weak self]in
            do{
                let json=try await aiEngineWork{try engine.dispatch(command:"paint.colors.describe",paramsJson:"{}")}
                let next=try JSONDecoder().decode(PaintColorDescription.self,from:Data(json.utf8))
                guard let self,self.ticket==request else{return};self.loadingVersion=nil
                guard next.version>=self.observedVersion else{return}
                self.state=next;self.error=""
            }catch{
                guard let self,self.ticket==request else{return};self.loadingVersion=nil
                self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription
            }
        }
    }
}

struct ColorPanelDraft:Equatable{
    var target="foreground"
    var model="rgb"
    var values=[0.0,0.0,0.0]
    var alpha=100.0
    var hex="#000000FF"
    init(){}
    init(state:PaintColorDescription,target:String,model:String,depth:UInt8){
        self.target=target;self.model=model
        let color=target=="background" ? state.background:state.foreground
        if model=="hsb"{values=color.hsb}
        else{values=depth==16 ? color.rgb16.prefix(3).map(Double.init):color.rgb8.prefix(3).map(Double.init)}
        alpha=color.rgba[3]*100;hex=color.hex
    }
    func colorInput(depth:UInt8)->[String:Any]?{
        if model=="hex"{
            let value=hex.trimmingCharacters(in:.whitespacesAndNewlines)
            guard value.range(of:"^#?([0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$",options:.regularExpression) != nil else{return nil}
            return ["model":"hex","value":value]
        }
        guard values.count==3,values.allSatisfy({$0.isFinite}),alpha.isFinite,(0...100).contains(alpha)else{return nil}
        if model=="hsb"{
            guard (0...360).contains(values[0]),values.dropFirst().allSatisfy({(0...100).contains($0)})else{return nil}
            return ["model":"hsb","hue":values[0],"saturation":values[1],"brightness":values[2],"alpha":alpha/100]
        }
        guard model=="rgb" else{return nil};let maxCode=depth==16 ? 65535.0:255.0
        guard values.allSatisfy({(0...maxCode).contains($0)&&$0.rounded()==$0})else{return nil}
        return ["model":depth==16 ? "rgb16":"rgb8","red":Int(values[0]),"green":Int(values[1]),"blue":Int(values[2]),"alpha":alpha/100]
    }
}
