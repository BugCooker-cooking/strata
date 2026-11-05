import SwiftUI

func tonalTitle(_ type: String) -> String {
    switch type { case "levels": "色阶"; case "curves": "曲线"; case "brightness_contrast": "亮度 / 对比度"; default: "调色调整层" }
}
struct TonalLevelRange: Codable, Equatable {
    var inputBlack = 0.0, inputWhite = 1.0, gamma = 1.0, outputBlack = 0.0, outputWhite = 1.0
}
struct TonalCurvePoint: Codable, Equatable {
    var x: Double, y: Double
    init(_ x: Double, _ y: Double) { self.x = x; self.y = y }
}
struct TonalDraft: Equatable {
    var type = "brightness_contrast"
    var brightness = 0.0, contrast = 0.0
    var levels = Array(repeating: TonalLevelRange(), count: 4)
    var curves = Array(repeating: [TonalCurvePoint(0,0), TonalCurvePoint(1,1)], count: 4)
    init(_ json: AIJSON? = nil) {
        guard let json else { return }
        type = json["type"].string ?? type
        brightness = json["brightness"].number ?? 0; contrast = json["contrast"].number ?? 0
        func range(_ json: AIJSON) -> TonalLevelRange {
            TonalLevelRange(inputBlack: json["input_black"].number ?? 0, inputWhite: json["input_white"].number ?? 1,
                gamma: json["gamma"].number ?? 1, outputBlack: json["output_black"].number ?? 0, outputWhite: json["output_white"].number ?? 1)
        }
        func points(_ json: AIJSON) -> [TonalCurvePoint] {
            let values = json.array.compactMap { value -> TonalCurvePoint? in
                let pair = value.array; guard pair.count == 2, let x = pair[0].number, let y = pair[1].number else { return nil }
                return TonalCurvePoint(x,y)
            }
            return values.count >= 2 ? values : [TonalCurvePoint(0,0), TonalCurvePoint(1,1)]
        }
        if type == "levels" {
            levels[0] = range(json["master"])
            for (i,value) in json["channels"].array.prefix(3).enumerated() { levels[i+1] = range(value) }
        } else if type == "curves" {
            curves[0] = points(json["master"])
            for (i,value) in json["channels"].array.prefix(3).enumerated() { curves[i+1] = points(value) }
        }
    }
    var parameters: [String: Any] {
        func range(_ value: TonalLevelRange) -> [String:Double] {
            ["input_black":value.inputBlack,"input_white":value.inputWhite,"gamma":value.gamma,"output_black":value.outputBlack,"output_white":value.outputWhite]
        }
        switch type {
        case "levels": return ["type":type,"master":range(levels[0]),"channels":levels.dropFirst().map(range)]
        case "curves": return ["type":type,"master":curves[0].map{[$0.x,$0.y]},"channels":curves.dropFirst().map{$0.map{[$0.x,$0.y]}}]
        default: return ["type":type,"brightness":brightness,"contrast":contrast]
        }
    }
}

struct TonalAdjustmentControls: View {
    @ObservedObject var model: WorkspaceModel
    @State private var draft = TonalDraft()
    @State private var channel = 0
    private var dirty: Bool { draft != TonalDraft(model.selected?.kind?.adjustment) }
    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(tonalTitle(draft.type)).fontWeight(.medium)
            if draft.type == "brightness_contrast" {
                row("亮度", $draft.brightness); row("对比度", $draft.contrast)
                Text("−100…100 · 当前工作色彩空间").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
            } else {
                Picker("调整通道", selection: $channel) {
                    Text("RGB").tag(0); Text("红").tag(1); Text("绿").tag(2); Text("蓝").tag(3)
                }.pickerStyle(.segmented)
                if draft.type == "levels" {
                    level("输入黑点", \.inputBlack); level("输入白点", \.inputWhite); level("中间调 Gamma", \.gamma)
                    level("输出黑点", \.outputBlack); level("输出白点", \.outputWhite)
                    Text("端点 0…1 · Gamma 0.1…10").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
                } else {
                    TonalCurveEditor(points: $draft.curves[channel])
                }
            }
            HStack {
                Button("应用调整") { commit() }.disabled(!dirty)
                Button("重置") { draft = TonalDraft(); draft.type = model.selected?.kind?.adjustment?["type"].string ?? "brightness_contrast" }
            }
            Text(dirty ? "参数尚未应用；应用后可撤销。" : "参数已保存到调整层。").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary))
        }.textFieldStyle(.roundedBorder)
            .disabled(!model.permissions.edits || model.selected?.locks?.all == true)
            .onAppear(perform: sync)
            .onChange(of: model.selected?.id) { _,_ in sync() }
            .onChange(of: model.selected?.kind?.adjustment) { _,_ in draft = TonalDraft(model.selected?.kind?.adjustment) }
    }
    private func row(_ title: String, _ value: Binding<Double>) -> some View {
        HStack { Text(title).font(.system(size: 11)); Spacer(); TextField(title,value:value,format:.number.precision(.fractionLength(3))).frame(width:80).accessibilityLabel(title) }
    }
    private func level(_ title: String, _ key: WritableKeyPath<TonalLevelRange,Double>) -> some View {
        row(title, Binding(get:{draft.levels[channel][keyPath:key]},set:{draft.levels[channel][keyPath:key]=$0}))
    }
    private func sync() { draft = TonalDraft(model.selected?.kind?.adjustment); channel = 0 }
    private func commit() {
        guard let layer = model.selected else { return }
        model.send(.command("adjust.tonal.set",["layer":["id":layer.id],"adjustment":draft.parameters]))
    }
}

struct TonalCurveEditor: View {
    @Binding var points: [TonalCurvePoint]
    @State private var dragged: Int?
    @State private var selected = 0
    private var samples: [Float] {
        guard let bytes = try? JSONEncoder().encode(points.map{[$0.x,$0.y]}) else { return [] }
        return (try? tonalCurvePreview(pointsJson:String(decoding:bytes,as:UTF8.self))) ?? []
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            GeometryReader { geometry in
                ZStack {
                    Path { p in
                        for i in 1...3 {let d=CGFloat(i)/4; p.move(to:CGPoint(x:d*geometry.size.width,y:0));p.addLine(to:CGPoint(x:d*geometry.size.width,y:geometry.size.height));p.move(to:CGPoint(x:0,y:d*geometry.size.height));p.addLine(to:CGPoint(x:geometry.size.width,y:d*geometry.size.height))}
                    }.stroke(Color(nsColor:WorkspaceColors.border),lineWidth:1)
                    Path { p in
                        let values=samples
                        for (i,v) in values.enumerated() {let point=CGPoint(x:CGFloat(i)/CGFloat(max(1,values.count-1))*geometry.size.width,y:(1-CGFloat(v))*geometry.size.height);if i==0{p.move(to:point)}else{p.addLine(to:point)}}
                    }.stroke(Color(nsColor:WorkspaceColors.accent),lineWidth:1.5)
                    ForEach(points.indices,id:\.self) { i in
                        Circle().fill(i==selected ? Color.white : Color(nsColor:WorkspaceColors.accent)).frame(width:8,height:8)
                            .position(x:points[i].x*geometry.size.width,y:(1-points[i].y)*geometry.size.height)
                    }
                }.contentShape(Rectangle()).gesture(DragGesture(minimumDistance:0).onChanged{value in
                    let x=min(1,max(0,value.location.x/geometry.size.width)),y=min(1,max(0,1-value.location.y/geometry.size.height))
                    if dragged==nil {
                        let nearest=points.indices.min{a,b in hypot(points[a].x*geometry.size.width-value.location.x,(1-points[a].y)*geometry.size.height-value.location.y)<hypot(points[b].x*geometry.size.width-value.location.x,(1-points[b].y)*geometry.size.height-value.location.y)} ?? 0
                        if hypot(points[nearest].x*geometry.size.width-value.location.x,(1-points[nearest].y)*geometry.size.height-value.location.y)>14,points.count<64,x>0.00001,x<0.99999{
                            let index=points.firstIndex(where:{$0.x>x}) ?? points.count-1;points.insert(TonalCurvePoint(x,y),at:index);dragged=index
                        }else{dragged=nearest}
                    }
                    guard let index=dragged,points.indices.contains(index) else{return};selected=index
                    if index>0,index+1<points.count {points[index].x=min(points[index+1].x-0.00001,max(points[index-1].x+0.00001,x))}
                    points[index].y=y
                }.onEnded{_ in dragged=nil})
            }.frame(height:160).background(Color(nsColor:WorkspaceColors.input)).accessibilityLabel("曲线控制图；拖动控制点，或单击增加点。下方可用键盘输入。")
            Picker("控制点",selection:$selected) {ForEach(points.indices,id:\.self){i in Text("点 \(i+1)").tag(i)}}
            if points.indices.contains(selected) {
                HStack {
                    Text("输入");TextField("曲线输入",value:$points[selected].x,format:.number.precision(.fractionLength(4))).disabled(selected==0 || selected+1==points.count)
                    Text("输出");TextField("曲线输出",value:$points[selected].y,format:.number.precision(.fractionLength(4)))
                }.font(.system(size:10))
                Button("移除控制点"){points.remove(at:selected);selected=max(0,selected-1)}.disabled(selected==0 || selected+1==points.count)
            }
            Button("添加中间控制点") {
                let left = min(selected,max(0,points.count-2)), right = left+1
                let point = TonalCurvePoint((points[left].x+points[right].x)/2,(points[left].y+points[right].y)/2)
                points.insert(point,at:right);selected=right
            }.disabled(points.count>=64)
            Text("输入/输出 0…1 · 应用前可继续编辑").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
        }.onChange(of:points.count){_,count in selected=min(selected,max(0,count-1));dragged=nil}
    }
}
