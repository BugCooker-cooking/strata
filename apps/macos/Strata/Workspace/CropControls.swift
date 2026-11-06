import SwiftUI

struct CropToolOptions:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var input:CanvasCropInput
    var body:some View{
        HStack(spacing:9){
            Picker("裁剪比例",selection:Binding(get:{model.cropRatio},set:{model.cropRatio=$0;model.send(.tool(.crop))})){Text("自由").tag("free");Text("1:1").tag("1:1");Text("4:3").tag("4:3");Text("3:2").tag("3:2");Text("16:9").tag("16:9")}.frame(width:100).disabled(input.running || !model.permissions.edits)
            if let rect=input.draft{Text("\(Int64(rect.minX)), \(Int64(rect.minY)) · \(Int(rect.width))×\(Int(rect.height)) px").monospacedDigit()}
            else{Text("拖动框内移动 · 手柄缩放 · 回车应用 · Esc 取消").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            Text("保留像素").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Button("应用裁剪"){input.confirm()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(input.draft == nil||input.running || !model.permissions.edits)
            Button("取消裁剪"){input.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!input.hasDraft && !input.running)
            if !input.error.isEmpty{Text(input.error).foregroundStyle(Color(nsColor:WorkspaceColors.danger)).lineLimit(1)}
        }
    }
}

struct CropPanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var x=0.0
    @State private var y=0.0
    @State private var width=1.0
    @State private var height=1.0
    @State private var ratio="free"
    private var signature:String{guard let f=model.snapshot?.frame else{return ""};return "\(model.snapshot?.documentId ?? ""):\(f.x):\(f.y):\(f.width):\(f.height)"}
    private var parameters:[String:Any]?{
        guard [x,y,width,height].allSatisfy({$0.isFinite&&$0.rounded()==$0}),abs(x)<=1e12,abs(y)<=1e12,(1...100000).contains(width),(1...100000).contains(height),let left=Int64(exactly:x),let top=Int64(exactly:y),let w=UInt32(exactly:width),let h=UInt32(exactly:height)else{return nil}
        var p:[String:Any]=["rect":["x":left,"y":top,"width":w,"height":h],"pixels":"preserve"]
        if ratio != "free"{let pair=ratio.split(separator:":").compactMap{UInt32($0)};guard pair.count==2 else{return nil};p["aspect_ratio"]=pair};return p
    }
    var body:some View{
        Group{
            if model.tool == .crop,let input=model.cropInput{CropDraftInspector(model:model,input:input)}
            else{numericControls}
        }.onAppear{readFrame()}.onChange(of:signature){_,_ in readFrame()}
    }
    private var numericControls:some View{
        VStack(alignment:.leading,spacing:8){
            Text("矩形裁剪 · 保留像素").fontWeight(.medium)
            Text("按画布坐标裁剪画板；边界外的原图、图层与蒙版会保留。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{field("裁剪 X",$x);field("裁剪 Y",$y)}
            HStack{field("裁剪宽度",$width);field("裁剪高度",$height)}
            Picker("比例",selection:$ratio){Text("自由").tag("free");Text("1:1").tag("1:1");Text("4:3").tag("4:3");Text("3:2").tag("3:2");Text("16:9").tag("16:9")}
            if ratio != "free"{Text("从左上角开始，在输入框范围内取符合比例的最大整数矩形。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            Button("应用裁剪"){if let parameters{model.send(.command("image.crop",parameters))}}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(parameters == nil)
            Button("显示全部可见图层内容"){model.send(.command("image.reveal_all",[:]))}.buttonStyle(WorkspaceButtonStyle())
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
    }
    private func field(_ title:String,_ value:Binding<Double>)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel(title)}}
    private func readFrame(){guard let f=model.snapshot?.frame else{return};x=Double(f.x);y=Double(f.y);width=Double(f.width);height=Double(f.height)}
}

private struct CropDraftInspector:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var input:CanvasCropInput
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("裁剪草稿 · 保留像素").fontWeight(.medium)
            if let rect=input.draft{
                Text("X \(Int64(rect.minX)) · Y \(Int64(rect.minY))").monospacedDigit()
                Text("\(Int(rect.width)) × \(Int(rect.height)) px").monospacedDigit()
                Text("草稿尚未写入文档；应用后可撤销。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }else{Text("在画布上拖出裁剪框；顶部可以选择比例。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            HStack{
                Button("应用裁剪草稿"){input.confirm()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(input.draft == nil || input.running || !model.permissions.edits)
                Button("取消裁剪草稿"){input.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!input.hasDraft && !input.running)
            }
            if !input.error.isEmpty{Text(input.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
        }
    }
}
