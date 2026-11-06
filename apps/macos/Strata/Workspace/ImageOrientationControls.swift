import SwiftUI

struct ImageOrientationPanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var operation="rotate_90_cw"
    @State private var showTrim=false
    @State private var arbitraryAngle=0.0
    @State private var interpolation="bilinear"
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("整幅图像旋转 / 翻转").fontWeight(.medium)
            Text("同步变换全部图层、蒙版与选区；保持可编辑，保留画板外内容。90° 旋转交换画板宽高。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Picker("方向",selection:$operation){Text("顺时针 90°").tag("rotate_90_cw");Text("逆时针 90°").tag("rotate_90_ccw");Text("旋转 180°").tag("rotate_180");Text("水平翻转").tag("flip_horizontal");Text("垂直翻转").tag("flip_vertical")}
            Button("应用整图方向"){model.send(.command("image.orient",["operation":operation]))}.buttonStyle(WorkspaceButtonStyle(selected:true))
            Divider()
            HStack{Text("任意角度 °");TextField("整图旋转角度",value:$arbitraryAngle,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel("整图旋转角度")}
            Picker("整图旋转重采样",selection:$interpolation){Text("双线性（线性光）").tag("bilinear");Text("最近邻").tag("nearest")}
            Text("任意角度当前支持像素层和无样式图层组；画板自动扩展，图层仍保持独立。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Button("应用整图角度"){model.send(.command("image.rotate",["angle_degrees":arbitraryAngle,"interpolation":interpolation]))}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!arbitraryAngle.isFinite||abs(arbitraryAngle)>360000)
            Button("裁去透明边…"){showTrim=true}.buttonStyle(WorkspaceButtonStyle()).popover(isPresented:$showTrim){TransparentTrimControls(model:model,presented:$showTrim)}
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
    }
}

private struct TransparentTrimControls:View{
    @ObservedObject var model:WorkspaceModel
    @Binding var presented:Bool
    @State private var top=true
    @State private var bottom=true
    @State private var left=true
    @State private var right=true
    var body:some View{
        VStack(alignment:.leading,spacing:10){
            Text("裁去透明边").fontWeight(.medium)
            Text("按可见合成的透明边界裁切画板，保留原始图层与像素。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{Toggle("顶部",isOn:$top);Toggle("底部",isOn:$bottom)}
            HStack{Toggle("左边",isOn:$left);Toggle("右边",isOn:$right)}
            Button("应用透明边裁切"){model.send(.command("image.trim",["top":top,"bottom":bottom,"left":left,"right":right]));presented=false}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!top && !bottom && !left && !right)
        }.padding(14).frame(width:240).disabled(!model.permissions.edits)
    }
}
