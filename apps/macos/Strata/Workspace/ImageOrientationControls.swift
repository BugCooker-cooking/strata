import SwiftUI

struct ImageOrientationPanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var operation="rotate_90_cw"
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("整幅图像旋转 / 翻转").fontWeight(.medium)
            Text("同步变换全部图层、蒙版与选区；保持可编辑，保留画板外内容。90° 旋转交换画板宽高。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Picker("方向",selection:$operation){Text("顺时针 90°").tag("rotate_90_cw");Text("逆时针 90°").tag("rotate_90_ccw");Text("旋转 180°").tag("rotate_180");Text("水平翻转").tag("flip_horizontal");Text("垂直翻转").tag("flip_vertical")}
            Button("应用整图方向"){model.send(.command("image.orient",["operation":operation]))}.buttonStyle(WorkspaceButtonStyle(selected:true))
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
    }
}
