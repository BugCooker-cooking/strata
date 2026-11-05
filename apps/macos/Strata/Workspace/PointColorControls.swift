import SwiftUI

struct PointGradientStop: Equatable {
    var at: Double
    var color: [Double]
}
struct PointColorDraft: Equatable {
    var type = "invert"
    var vibrance = 0.0, saturation = 0.0
    var temperature = 0.0, tintShift = 0.0
    var shadowsAmount = 0.0, highlightsAmount = 0.0, shadowsWidth = 0.5, highlightsWidth = 0.5, localRadius = 30.0
    var balance = Array(repeating: [0.0,0.0,0.0],count:3)
    var preserveLuminance = true
    var weights = [40.0,60,40,60,20,80]
    var tinted = false, tint = [0.7,0.5,0.3], tintAmount = 25.0
    var color = [1.0,0.65,0.3], density = 25.0
    var matrix = [[1.0,0,0,0],[0,1,0,0],[0,0,1,0]], gray = [0.3,0.59,0.11,0.0]
    var monochrome = false
    var levels = 6, threshold = 0.5
    var stops = [PointGradientStop(at:0,color:[0,0,0]),PointGradientStop(at:1,color:[1,1,1])]
    var reverse = false, relative = true
    var ranges = Array(repeating:[0.0,0.0,0.0,0.0],count:9)
    init(_ json: AIJSON? = nil) {
        guard let json else { return }
        type=json["type"].string ?? type
        func vector(_ value:AIJSON,_ fallback:[Double])->[Double]{let values=value.array.compactMap(\.number);return values.count==fallback.count ? values : fallback}
        vibrance=json["vibrance"].number ?? 0;saturation=json["saturation"].number ?? 0
        temperature=json["temperature"].number ?? 0;tintShift=json["tint"].number ?? 0
        shadowsAmount=json["shadows"].number ?? 0;highlightsAmount=json["highlights"].number ?? 0
        shadowsWidth=json["shadows_width"].number ?? 0.5;highlightsWidth=json["highlights_width"].number ?? 0.5;localRadius=json["radius"].number ?? 30
        for (i,key) in ["shadows","midtones","highlights"].enumerated(){balance[i]=vector(json[key],balance[i])}
        preserveLuminance=json["preserve_luminance"].bool ?? true
        weights=vector(json["weights"],weights);tint=vector(json["tint"],tint);tinted=json["tint"].array.count==3;tintAmount=json["tint_amount"].number ?? 25
        color=vector(json["color"],color);density=json["density"].number ?? 25
        for (i,row) in json["matrix"].array.prefix(3).enumerated(){matrix[i]=vector(row,matrix[i])}
        gray=vector(json["gray"],gray);monochrome=json["monochrome"].bool ?? false
        if let number=json["levels"].number,number.isFinite,(2...65536).contains(number){levels=Int(number)}
        threshold=json["level"].number ?? 0.5;reverse=json["reverse"].bool ?? false;relative=json["relative"].bool ?? true
        for (i,row) in json["ranges"].array.prefix(9).enumerated(){ranges[i]=vector(row,ranges[i])}
        let parsed=json["stops"].array.compactMap{value->PointGradientStop? in
            guard let at=value["at"].number,value["color"].array.count==3 else{return nil}
            return PointGradientStop(at:at,color:vector(value["color"],[0,0,0]))
        }
        if parsed.count>=2{stops=parsed}
    }
    var parameters:[String:Any]{
        switch type{
        case "vibrance":return ["type":type,"vibrance":vibrance,"saturation":saturation]
        case "white_balance":return ["type":type,"temperature":temperature,"tint":tintShift,"preserve_luminance":preserveLuminance]
        case "shadows_highlights":return ["type":type,"shadows":shadowsAmount,"highlights":highlightsAmount,"shadows_width":shadowsWidth,"highlights_width":highlightsWidth,"radius":localRadius]
        case "color_balance":return ["type":type,"shadows":balance[0],"midtones":balance[1],"highlights":balance[2],"preserve_luminance":preserveLuminance]
        case "black_white":var p:[String:Any]=["type":type,"weights":weights,"tint_amount":tintAmount];p["tint"]=tinted ? tint as Any : NSNull();return p
        case "photo_filter":return ["type":type,"color":color,"density":density,"preserve_luminance":preserveLuminance]
        case "channel_mixer":return ["type":type,"matrix":matrix,"gray":gray,"monochrome":monochrome]
        case "posterize":return ["type":type,"levels":levels]
        case "threshold":return ["type":type,"level":threshold]
        case "gradient_map":return ["type":type,"stops":stops.map{["at":$0.at,"color":$0.color] as [String:Any]},"reverse":reverse]
        case "selective_color":return ["type":type,"ranges":ranges,"relative":relative]
        default:return ["type":type]
        }
    }
}

struct PointColorControls:View {
    @ObservedObject var model:WorkspaceModel
    @State private var draft=PointColorDraft()
    @State private var rangeIndex=0
    @State private var mixerRow=0
    @State private var stopIndex=0
    private var dirty:Bool{draft != PointColorDraft(model.selected?.kind?.adjustment)}
    var body:some View{
        VStack(alignment:.leading,spacing:9){
            Text(tonalTitle(draft.type)).fontWeight(.medium)
            controls
            HStack{
                Button("应用调整"){commit()}.disabled(!dirty)
                Button("重置"){let type=draft.type;draft=PointColorDraft();draft.type=type;rangeIndex=0;mixerRow=0;stopIndex=0}
            }
            if let preview=model.tonalPreview {TonalPreviewControls(preview:preview,parameters:try? commandJSON(draft.parameters))}
            Text(dirty ? "参数尚未应用；应用后可撤销。" : "参数已保存到调整层。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
        }.textFieldStyle(.roundedBorder).disabled(!model.permissions.edits || model.selected?.locks?.all == true)
            .onAppear(perform:sync).onChange(of:model.selected?.id){_,_ in sync()}
            .onChange(of:model.selected?.kind?.adjustment){_,_ in draft=PointColorDraft(model.selected?.kind?.adjustment);stopIndex=min(stopIndex,draft.stops.count-1)}
    }
    @ViewBuilder private var controls:some View{
        switch draft.type{
        case "vibrance":
            row("自然饱和度",$draft.vibrance);row("饱和度",$draft.saturation);note("−100…100 · 自然饱和度优先增强低饱和区域并减弱肤色范围。")
        case "white_balance":
            row("色温偏移",$draft.temperature);row("色调偏移",$draft.tintShift);Toggle("保持亮度",isOn:$draft.preserveLuminance)
            note("−100…100 · 色温正值偏暖，负值偏冷；色调正值偏洋红，负值偏绿。")
        case "shadows_highlights":
            row("提亮阴影 %",$draft.shadowsAmount);row("压低高光 %",$draft.highlightsAmount)
            row("阴影范围",$draft.shadowsWidth);row("高光范围",$draft.highlightsWidth);row("局部半径 px",$draft.localRadius)
            note("强度 0…100 · 范围 0.01…1 · 半径 0…256。调整局部明暗；原始图层保持可编辑。")
        case "color_balance":
            Picker("色调范围",selection:$rangeIndex){Text("阴影").tag(0);Text("中间调").tag(1);Text("高光").tag(2)}.pickerStyle(.segmented)
            vector($draft.balance[rangeIndex],labels:["青色 ↔ 红色","洋红 ↔ 绿色","黄色 ↔ 蓝色"])
            Toggle("保持亮度",isOn:$draft.preserveLuminance);note("−100…100 · 正值增强红 / 绿 / 蓝。")
        case "black_white":
            vector($draft.weights,labels:["红色","黄色","绿色","青色","蓝色","洋红"])
            Toggle("着色",isOn:$draft.tinted)
            if draft.tinted{color("着色颜色",$draft.tint);row("着色强度 %",$draft.tintAmount)}
            note("六类色相响应 −200…300 · 着色强度 0…100。")
        case "photo_filter":
            color("滤镜颜色",$draft.color);row("浓度 %",$draft.density);Toggle("保持亮度",isOn:$draft.preserveLuminance);note("浓度 0…100 · 颜色使用当前工作 ICC。")
        case "channel_mixer":
            Toggle("单色",isOn:$draft.monochrome)
            if draft.monochrome{vector($draft.gray,labels:["红色系数","绿色系数","蓝色系数","常数"])}
            else{
                Picker("输出通道",selection:$mixerRow){Text("红").tag(0);Text("绿").tag(1);Text("蓝").tag(2)}.pickerStyle(.segmented)
                vector($draft.matrix[mixerRow],labels:["红色系数","绿色系数","蓝色系数","常数"])
            }
            note("系数 −2…2 · 1 代表 100%。")
        case "posterize":
            HStack{Text("色阶数");TextField("色阶数",value:$draft.levels,format:.number)};note("每通道 2…65536 级，保持文档位深。")
        case "threshold":row("亮度阈值",$draft.threshold);note("0…1 · 高于或等于阈值的区域变白。")
        case "gradient_map":gradient
        case "selective_color":
            Picker("颜色范围",selection:$rangeIndex){ForEach(Array(["红","黄","绿","青","蓝","洋红","白","中性色","黑"].enumerated()),id:\.offset){i,title in Text(title).tag(i)}}
            vector($draft.ranges[rangeIndex],labels:["青色 %","洋红 %","黄色 %","黑色 %"])
            Toggle("相对",isOn:$draft.relative);note("−100…100 · 关闭相对时使用绝对通道偏移。")
        case "invert":note("将编码工作 RGB 的每个通道反相，保留 alpha。")
        case "desaturate":note("使用 HSL 明度去色；保留原图层，可用蒙版限制区域。")
        default:EmptyView()
        }
    }
    private var gradient:some View{
        VStack(alignment:.leading,spacing:8){
            Toggle("反向",isOn:$draft.reverse)
            Picker("渐变色标",selection:$stopIndex){ForEach(draft.stops.indices,id:\.self){i in Text("色标 \(i+1)").tag(i)}}
            if draft.stops.indices.contains(stopIndex){
                row("位置",$draft.stops[stopIndex].at).disabled(stopIndex==0 || stopIndex+1==draft.stops.count)
                color("颜色",$draft.stops[stopIndex].color)
            }
            HStack{
                Button("添加色标"){
                    let a=min(stopIndex,draft.stops.count-2),b=a+1
                    let next=PointGradientStop(at:(draft.stops[a].at+draft.stops[b].at)/2,color:(0..<3).map{(draft.stops[a].color[$0]+draft.stops[b].color[$0])/2})
                    draft.stops.insert(next,at:b);stopIndex=b
                }.disabled(draft.stops.count>=64)
                Button("移除色标"){draft.stops.remove(at:stopIndex);stopIndex=max(0,stopIndex-1)}.disabled(stopIndex==0 || stopIndex+1==draft.stops.count)
            }
            note("位置 0…1 且严格递增；按工作 RGB 亮度映射并在编码 RGB 中插值。")
        }
    }
    private func row(_ title:String,_ value:Binding<Double>)->some View{
        HStack{Text(title).font(.system(size:11));Spacer();TextField(title,value:value,format:.number.precision(.fractionLength(3))).frame(width:76).accessibilityLabel(title)}
    }
    private func vector(_ values:Binding<[Double]>,labels:[String])->some View{
        VStack(spacing:7){ForEach(labels.indices,id:\.self){i in row(labels[i],Binding(get:{values.wrappedValue[i]},set:{values.wrappedValue[i]=$0}))}}
    }
    private func note(_ text:String)->some View{Text(text).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary)).fixedSize(horizontal:false,vertical:true)}
    private func color(_ title:String,_ value:Binding<[Double]>)->some View{
        ColorPicker(title,selection:Binding(get:{
            var components=value.wrappedValue.map{CGFloat($0)};components.append(1)
            return Color(nsColor:NSColor(colorSpace:model.workingColorSpace,components:&components,count:4))
        },set:{color in
            if let converted=NSColor(color).usingColorSpace(model.workingColorSpace){value.wrappedValue=[converted.redComponent,converted.greenComponent,converted.blueComponent]}
        }),supportsOpacity:false).accessibilityLabel(title)
    }
    private func sync(){draft=PointColorDraft(model.selected?.kind?.adjustment);rangeIndex=0;mixerRow=0;stopIndex=0}
    private func commit(){guard let layer=model.selected else{return};if let preview=model.tonalPreview,preview.enabled,let parameters=try? commandJSON(draft.parameters){preview.apply(parameters);return};model.send(.command("adjust.tonal.set",["layer":["id":layer.id],"adjustment":draft.parameters]))}
}
