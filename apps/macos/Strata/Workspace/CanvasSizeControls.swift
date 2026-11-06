import SwiftUI

struct CanvasSizePanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var width=1.0
    @State private var height=1.0
    @State private var anchor="center"
    private let anchors:[(String,String)]=[("左上","top_left"),("上","top"),("右上","top_right"),("左","left"),("中心","center"),("右","right"),("左下","bottom_left"),("下","bottom"),("右下","bottom_right")]
    private var frameSignature:String{
        guard let f=model.snapshot?.frame else{return ""}
        return "\(model.snapshot?.documentId ?? ""):\(f.x):\(f.y):\(f.width):\(f.height)"
    }
    private var valid:Bool{[width,height].allSatisfy{$0.isFinite&&$0>=1&&$0<=100000&&$0.rounded()==$0}}
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("画布大小 · 保留内容").fontWeight(.medium)
            Text("只改变画板边界；不重采样，不删除边界外的图层与蒙版。扩展区域保持透明，已有内容会显示。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{
                Text("宽");TextField("画布宽度",value:$width,format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel("画布宽度")
                Text("高");TextField("画布高度",value:$height,format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel("画布高度")
            }
            Picker("固定锚点",selection:$anchor){ForEach(anchors,id:\.1){item in Text(item.0).tag(item.1)}}
            Button("应用画布大小"){model.send(.command("image.canvas_size",["width":Int(width),"height":Int(height),"anchor":anchor]))}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!valid)
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
            .onAppear{readFrame()}.onChange(of:frameSignature){_,_ in readFrame()}
    }
    private func readFrame(){guard let f=model.snapshot?.frame else{return};width=Double(f.width);height=Double(f.height)}
}
