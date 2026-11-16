import SwiftUI

struct CanvasSizePanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var width=1.0
    @State private var height=1.0
    @State private var anchor="center"
    private let anchors:[(String,String)]=[(L10n.text("左上"),"top_left"),(L10n.text("上"),"top"),(L10n.text("右上"),"top_right"),(L10n.text("左"),"left"),(L10n.text("中心"),"center"),(L10n.text("右"),"right"),(L10n.text("左下"),"bottom_left"),(L10n.text("下"),"bottom"),(L10n.text("右下"),"bottom_right")]
    private var frameSignature:String{
        guard let f=model.snapshot?.frame else{return ""}
        return "\(model.snapshot?.documentId ?? ""):\(f.x):\(f.y):\(f.width):\(f.height)"
    }
    private var valid:Bool{[width,height].allSatisfy{$0.isFinite&&$0>=1&&$0<=100000&&$0.rounded()==$0}}
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text(L10n.text("画布大小 · 保留内容")).fontWeight(.medium)
            Text(L10n.text("只改变画板边界；不重采样，不删除边界外的图层与蒙版。扩展区域保持透明，已有内容会显示。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{
                Text(L10n.text("宽"));TextField(L10n.text("画布宽度"),value:$width,format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("画布宽度"))
                Text(L10n.text("高"));TextField(L10n.text("画布高度"),value:$height,format:.number.precision(.fractionLength(0))).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("画布高度"))
            }
            Picker(L10n.text("固定锚点"),selection:$anchor){ForEach(anchors,id:\.1){item in Text(item.0).tag(item.1)}}
            Button(L10n.text("应用画布大小")){model.send(.command("image.canvas_size",["width":Int(width),"height":Int(height),"anchor":anchor]))}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!valid)
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
            .onAppear{readFrame()}.onChange(of:frameSignature){_,_ in readFrame()}
    }
    private func readFrame(){guard let f=model.snapshot?.frame else{return};width=Double(f.width);height=Double(f.height)}
}
