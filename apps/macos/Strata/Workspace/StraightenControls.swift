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
        VStack(alignment:.leading,spacing:8){
            Text("拉直图像 · 保留源内容").fontWeight(.medium)
            Text("按角度或两点参考线校正；自动裁掉旋转空角，边界外内容保留，一次撤销恢复旋转与裁剪。当前支持像素层和无样式组。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Picker("拉直依据",selection:$mode){Text("校正角度").tag("angle");Text("两点参考线").tag("line")}
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
            .onAppear{readLine()}.onChange(of:signature){_,_ in readLine()}
    }
    private func field(_ title:String,_ value:Binding<Double>)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel(title)}}
    private func readLine(){guard let f=model.snapshot?.frame else{return};startX=Double(f.x)+Double(f.width)*0.1;endX=Double(f.x)+Double(f.width)*0.9;startY=Double(f.y)+Double(f.height)/2;endY=startY}
}
