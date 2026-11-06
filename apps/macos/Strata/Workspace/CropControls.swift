import SwiftUI

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
            .onAppear{readFrame()}.onChange(of:signature){_,_ in readFrame()}
    }
    private func field(_ title:String,_ value:Binding<Double>)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel(title)}}
    private func readFrame(){guard let f=model.snapshot?.frame else{return};x=Double(f.x);y=Double(f.y);width=Double(f.width);height=Double(f.height)}
}
