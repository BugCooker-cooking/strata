import SwiftUI

struct ImageSizePanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var width=1.0
    @State private var height=1.0
    @State private var linked=true
    @State private var interpolation="area"
    private var signature:String{"\(model.snapshot?.documentId ?? ""):\(model.snapshot?.frame?.width ?? 0):\(model.snapshot?.frame?.height ?? 0)"}
    static func linkedSize(_ value:Double,from:UInt32,to:UInt32)->Double?{
        guard value.isFinite,value>=1,value<=100000,from>0,to>0 else{return nil}
        let other=(value*Double(to)/Double(from)).rounded()
        return max(1,other)
    }
    private var valid:Bool{[width,height].allSatisfy{$0.isFinite&&$0>=1&&$0<=100000&&$0.rounded()==$0} && width*height<=64000000}
    private func dimension(_ horizontal:Bool)->Binding<Double>{
        Binding(get:{horizontal ? width:height},set:{value in
            if horizontal{width=value}else{height=value}
            if linked,let f=model.snapshot?.frame,let other=Self.linkedSize(value,from:horizontal ? f.width:f.height,to:horizontal ? f.height:f.width){if horizontal{height=other}else{width=other}}
        })
    }
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("图像大小 · 重采样").fontWeight(.medium)
            Text("同步缩放像素层、组、蒙版与选区，保持图层独立和画板左上角。当前不支持生成填充、调整或样式层。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{
                Text("宽");TextField("图像宽度",value:dimension(true),format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel("图像宽度")
                Text("高");TextField("图像高度",value:dimension(false),format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel("图像高度")
            }
            Toggle("约束图像比例",isOn:$linked).onChange(of:linked){_,value in if value{dimension(true).wrappedValue=width}}
            Picker("图像重采样",selection:$interpolation){Text("面积缩小 / 双线性放大").tag("area");Text("双线性（线性光）").tag("bilinear");Text("最近邻（保留原码）").tag("nearest")}
            Button("应用图像大小"){guard valid else{return};model.send(.command("image.resize",["width":Int(width),"height":Int(height),"interpolation":interpolation]))}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!valid)
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
            .onAppear{readFrame()}.onChange(of:signature){_,_ in readFrame()}
    }
    private func readFrame(){guard let f=model.snapshot?.frame else{return};width=Double(f.width);height=Double(f.height)}
}
