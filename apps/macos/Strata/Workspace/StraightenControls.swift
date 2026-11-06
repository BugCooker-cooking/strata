import SwiftUI

struct StraightenPanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var mode="angle"
    @State private var angle=0.0
    @State private var startX=0.0
    @State private var startY=0.0
    @State private var endX=100.0
    @State private var endY=0.0
    @State private var align="horizontal"
    @State private var crop=true
    @State private var interpolation="bilinear"
    private var signature:String{guard let f=model.snapshot?.frame else{return ""};return "\(model.snapshot?.documentId ?? ""):\(f.x):\(f.y):\(f.width):\(f.height)"}
    private var parameters:[String:Any]?{
        let reference:[String:Any]
        if mode=="angle"{guard angle.isFinite,abs(angle)<=45 else{return nil};reference=["type":"angle","angle_degrees":angle]}
        else{guard [startX,startY,endX,endY].allSatisfy({$0.isFinite&&abs($0)<=1e12}),hypot(endX-startX,endY-startY)>=0.000001 else{return nil};reference=["type":"line","start":[startX,startY],"end":[endX,endY],"align":align]}
        return ["reference":reference,"crop":crop,"interpolation":interpolation]
    }
    var body:some View{
        Group{
            if model.tool == .crop && model.cropMode=="straighten",let input=model.straightenInput{StraightenDraftInspector(model:model,input:input)}
            else{numericControls}
        }.onAppear{readLine()}.onChange(of:signature){_,_ in readLine()}
    }
    private var numericControls:some View{
        VStack(alignment:.leading,spacing:8){
            Text("拉直图像 · 保留源内容").fontWeight(.medium)
            Text("按角度或两点参考线校正；自动裁掉旋转空角，边界外内容保留，一次撤销恢复旋转与裁剪。当前支持像素层和无样式组。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Picker("拉直依据",selection:$mode){Text("校正角度").tag("angle");Text("两点参考线").tag("line")}
            Button("在画布拖出拉直参考线"){model.cropMode="straighten";model.inspector = .transform;model.selectTool(.crop)}.buttonStyle(WorkspaceButtonStyle())
            if mode=="angle"{field("拉直角度 °",$angle)}
            else{
                HStack{field("参考线起点 X",$startX);field("参考线起点 Y",$startY)}
                HStack{field("参考线终点 X",$endX);field("参考线终点 Y",$endY)}
                Picker("参考线对齐",selection:$align){Text("水平").tag("horizontal");Text("垂直").tag("vertical")}
                Text("输入画布坐标，校正不超过 ±45°；陡线请选择垂直对齐。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }
            Toggle("自动裁掉旋转空角",isOn:$crop)
            Picker("拉直重采样",selection:$interpolation){Text("双线性（线性光）").tag("bilinear");Text("最近邻").tag("nearest")}
            Button("应用拉直"){if let parameters{model.send(.command("image.straighten",parameters))}}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(parameters == nil)
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
    }
    private func field(_ title:String,_ value:Binding<Double>)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel(title)}}
    private func readLine(){guard let f=model.snapshot?.frame else{return};startX=Double(f.x)+Double(f.width)*0.1;endX=Double(f.x)+Double(f.width)*0.9;startY=Double(f.y)+Double(f.height)/2;endY=startY}
}

struct StraightenToolOptions:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var input:CanvasStraightenInput
    var body:some View{
        HStack(spacing:9){
            Picker("拉直对齐方向",selection:Binding(get:{model.straightenAlign},set:{model.straightenAlign=$0;model.send(.tool(.crop))})){Text("水平").tag("horizontal");Text("垂直").tag("vertical")}.labelsHidden().accessibilityLabel("拉直对齐方向").frame(width:100)
            Toggle("自动裁掉空角",isOn:Binding(get:{model.straightenAutoCrop},set:{model.straightenAutoCrop=$0;model.send(.tool(.crop))}))
            Picker("参考线重采样",selection:Binding(get:{model.straightenInterpolation},set:{model.straightenInterpolation=$0;model.send(.tool(.crop))})){Text("双线性").tag("bilinear");Text("最近邻").tag("nearest")}.labelsHidden().accessibilityLabel("参考线重采样").frame(width:110)
            if let angle=input.correction{Text("校正 \(angle,format:.number.precision(.fractionLength(2)))°").monospacedDigit()}
            else if input.draft == nil{Text("拖出参考线 · 回车应用 · Esc 取消").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            Button("应用参考线拉直"){input.confirm()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!input.canConfirm)
            Button("取消参考线"){input.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!input.hasDraft)
            if !input.error.isEmpty{Text(input.error).foregroundStyle(Color(nsColor:WorkspaceColors.danger)).lineLimit(1)}
        }.disabled(input.running || !model.permissions.edits)
    }
}

private struct StraightenDraftInspector:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var input:CanvasStraightenInput
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("画布参考线拉直").fontWeight(.medium)
            if let line=input.draft{
                Text("起点 \(line.start.x,format:.number.precision(.fractionLength(2))), \(line.start.y,format:.number.precision(.fractionLength(2)))").monospacedDigit()
                Text("终点 \(line.end.x,format:.number.precision(.fractionLength(2))), \(line.end.y,format:.number.precision(.fractionLength(2)))").monospacedDigit()
                if let angle=input.correction{Text("校正 \(angle,format:.number.precision(.fractionLength(2)))°").monospacedDigit()}
            }else{Text("在画布拖出沿地平线或建筑边缘的参考线；顶部选择水平或垂直对齐。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            Text("参考线尚未修改文档；应用后一次撤销恢复拉直与裁剪。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{
                Button("应用画布参考线"){input.confirm()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!input.canConfirm || !model.permissions.edits)
                Button("取消画布参考线"){input.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!input.hasDraft)
            }.disabled(input.running)
            if !input.error.isEmpty{Text(input.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
        }
    }
}
