import AppKit
import SwiftUI

@MainActor
final class TextEditingModel:ObservableObject{
    @Published var content="文字"
    @Published var font="sans"
    @Published var fontSize=32.0
    @Published var lineHeight=1.2
    @Published var tracking=0.0
    @Published var align="left"
    @Published var layout="point"
    @Published var width=240.0
    @Published var height=120.0
    @Published var x=0.0
    @Published var y=0.0
    @Published var red=0.0
    @Published var green=0.0
    @Published var blue=0.0
    @Published var alpha=100.0
    @Published private(set) var creating=true
    @Published private(set) var running=false
    @Published private(set) var error=""
    @Published private(set) var placementID=UUID()
    private let engine:StrataEngine
    private var snapshot:EditorSnapshot?
    private var target:String?
    private var generation=UUID()
    var onRunning:((Bool)->Void)?
    var onCommitted:(()->Void)?
    init(engine:StrataEngine){self.engine=engine}
    func bind(_ value:EditorSnapshot){
        if snapshot?.documentId != value.documentId || snapshot?.revision != value.revision || snapshot?.activeLayer != value.activeLayer || snapshot?.control?.kind != value.control?.kind{
            generation=UUID();snapshot=value;loadCurrent();return
        }
        snapshot=value
    }
    private func loadCurrent(){
        error="";target=nil;creating=true
        guard let snapshot,let layer=snapshot.layers.first(where:{$0.id==snapshot.activeLayer}),let value=layer.kind?.text,layer.kind?.kind=="text"else{return}
        target=layer.id;creating=false;let spec=value["spec"]
        content=spec["text"].string ?? "";font=spec["font"].string ?? "sans";fontSize=spec["font_size"].number ?? 32;lineHeight=spec["line_height"].number ?? 1.2;tracking=spec["tracking"].number ?? 0;align=spec["align"].string ?? "left";layout=spec["layout"]["type"].string ?? "point";width=spec["layout"]["width"].number ?? 240;height=spec["layout"]["height"].number ?? 120;x=Double(layer.x);y=Double(layer.y)
        let color=value["color"].array.compactMap(\.number),max=snapshot.bitDepth==16 ? 65535.0:255.0
        if color.count==4{red=color[0]/max*100;green=color[1]/max*100;blue=color[2]/max*100;alpha=color[3]/max*100}
    }
    func newText(at point:NSPoint?=nil,rect:CGRect?=nil){
        guard !running else{return};generation=UUID();target=nil;creating=true;error=""
        if let rect{layout="paragraph";x=Double(rect.minX.rounded(.down));y=Double(rect.minY.rounded(.down));width=Double(rect.width.rounded(.up));height=Double(rect.height.rounded(.up))}
        else if let point{layout="point";x=Double(point.x.rounded());y=Double(point.y.rounded())}
        else if let frame=snapshot?.frame{x=Double(frame.x)+32;y=Double(frame.y)+max(32,fontSize)}
        placementID=UUID()
    }
    func useForeground(){guard let color=snapshot?.paintColors?.foreground,color.count==4 else{return};red=color[0]*100;green=color[1]*100;blue=color[2]*100;alpha=color[3]*100}
    var parameters:[String:Any]?{
        guard content.utf8.count<=65536,content.unicodeScalars.count<=16384,[fontSize,lineHeight,tracking,red,green,blue,alpha,x,y].allSatisfy({$0.isFinite}),[red,green,blue,alpha].allSatisfy({(0...100).contains($0)}),(1...2048).contains(fontSize),(0.5...10).contains(lineHeight),(-256...2048).contains(tracking),tracking>=(-fontSize/2)else{return nil}
        var area:[String:Any]=["type":layout]
        if layout=="paragraph"{guard width.isFinite,height.isFinite,let w=UInt32(exactly:width),let h=UInt32(exactly:height),(1...100000).contains(w),(1...100000).contains(h)else{return nil};area["width"]=w;area["height"]=h}
        let spec:[String:Any]=["text":content,"font":font,"font_size":fontSize,"line_height":lineHeight,"tracking":tracking,"align":align,"layout":area]
        var parameters:[String:Any]=["spec":spec,"color":[red/100,green/100,blue/100,alpha/100]]
        if creating{guard let px=Int64(exactly:x),let py=Int64(exactly:y),abs(x)<=1e12,abs(y)<=1e12 else{return nil};parameters["name"]=String(content.prefix(32)).isEmpty ? "文字":String(content.prefix(32));parameters["x"]=px;parameters["y"]=py}
        else{guard let target else{return nil};parameters["layer"]=["id":target]}
        return parameters
    }
    var canApply:Bool{!running && snapshot?.documentId != nil && (snapshot?.control?.kind ?? "user")=="user" && parameters != nil}
    func revert(){guard !running else{return};generation=UUID();if creating{content="文字";font="sans";fontSize=32;lineHeight=1.2;tracking=0;align="left";layout="point";width=240;height=120;newText();useForeground()}else{loadCurrent()}}
    func apply(){
        guard canApply,let snapshot,let document=snapshot.documentId,let parameters else{return}
        do{
            let json=try commandJSON(parameters),command=creating ? "type.create":"type.update",engine=engine,revision=snapshot.revision,active=snapshot.activeLayer,current=generation;error="";running=true;onRunning?(true)
            Task{[weak self]in
                let result:Result<String,Error>
                do{result = .success(try await aiEngineWork{try engine.dispatchText(command:command,paramsJson:json,documentId:document,revision:revision,activeLayer:active)})}catch{result = .failure(error)}
                guard let self else{return};self.running=false;self.onRunning?(false)
                switch result{case .success:self.onCommitted?();case .failure(let error):if self.generation==current{self.error=(error as? StrataError)?.userMessage ?? error.localizedDescription}}
            }
        }catch{self.error=error.localizedDescription}
    }
}

struct TextEditingPanel:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var text:TextEditingModel
    @FocusState private var editingContent:Bool
    var body:some View{
        ScrollView{VStack(alignment:.leading,spacing:8){
            Text(text.creating ? "新建可编辑文字":"编辑文字图层").fontWeight(.medium)
            TextEditor(text:$text.content).font(.system(size:13)).frame(minHeight:90,idealHeight:120).accessibilityLabel("文字内容").focused($editingContent)
            Picker("字体",selection:$text.font){Text("Noto Sans").tag("sans");Text("Noto Serif").tag("serif");Text("Noto Sans CJK SC").tag("cjk")}
            number("字号",value:$text.fontSize,unit:"px")
            number("行距",value:$text.lineHeight,unit:"em")
            number("字距",value:$text.tracking,unit:"px")
            Picker("文字对齐",selection:$text.align){Text("左对齐").tag("left");Text("居中").tag("center");Text("右对齐").tag("right");Text("两端对齐").tag("justified")}
            Picker("文字类型",selection:$text.layout){Text("点文字").tag("point");Text("段落文字").tag("paragraph")}
            if text.layout=="paragraph"{number("段落宽",value:$text.width,unit:"px");number("段落高",value:$text.height,unit:"px")}
            if text.creating{number("文字 X",value:$text.x,unit:"px");number("文字 Y",value:$text.y,unit:"px")}
            HStack{Button("使用前景色"){text.useForeground()};Button("新建文字"){text.newText()}}.buttonStyle(WorkspaceButtonStyle())
            HStack{Text("文字颜色");TextField("文字红",value:$text.red,format:.number.precision(.fractionLength(2)));TextField("文字绿",value:$text.green,format:.number.precision(.fractionLength(2)));TextField("文字蓝",value:$text.blue,format:.number.precision(.fractionLength(2)))}.textFieldStyle(.roundedBorder)
            number("文字透明度",value:$text.alpha,unit:"%")
            Text("颜色按当前工作 RGB 百分比；点文字 Y 为首行基线，段落为框左上角。溢出内容保留，可增大段落框显示。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{Button(text.creating ? "创建文字":"应用文字修改"){editingContent=false;text.apply()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!text.canApply);Button(text.creating ? "恢复默认":"恢复当前值"){text.revert()}.buttonStyle(WorkspaceButtonStyle())}
            Text("修改在应用后更新画布，保留已有几何与删除裁剪，可一次撤销。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            if !text.error.isEmpty{Text(text.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.danger)).textSelection(.enabled)}
            if text.parameters==nil{Text("请检查字号、行距、字距、颜色及整数段落尺寸。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
        }.padding(12).disabled(!model.permissions.edits || text.running)}.onChange(of:text.placementID){_,_ in editingContent=true}.onChange(of:text.layout){_,_ in model.send(.configureText)}
    }
    private func number(_ title:String,value:Binding<Double>,unit:String)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel(title);Text(unit)}}
}
