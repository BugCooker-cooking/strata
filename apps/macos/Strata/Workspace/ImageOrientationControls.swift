import SwiftUI

struct ImageOrientationPanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var operation="rotate_90_cw"
    @State private var showTrim=false
    @State private var arbitraryAngle=0.0
    @State private var interpolation="bilinear"
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text(L10n.text("整幅图像旋转 / 翻转")).fontWeight(.medium)
            Text(L10n.text("同步变换全部图层、蒙版与选区；保持可编辑，保留画板外内容。90° 旋转交换画板宽高。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Picker(L10n.text("方向"),selection:$operation){Text(L10n.text("顺时针 90°")).tag("rotate_90_cw");Text(L10n.text("逆时针 90°")).tag("rotate_90_ccw");Text(L10n.text("旋转 180°")).tag("rotate_180");Text(L10n.text("水平翻转")).tag("flip_horizontal");Text(L10n.text("垂直翻转")).tag("flip_vertical")}
            Button(L10n.text("应用整图方向")){model.send(.command("image.orient",["operation":operation]))}.buttonStyle(WorkspaceButtonStyle(selected:true))
            Divider()
            HStack{Text(L10n.text("任意角度 °"));TextField(L10n.text("整图旋转角度"),value:$arbitraryAngle,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("整图旋转角度"))}
            Picker(L10n.text("整图旋转重采样"),selection:$interpolation){Text(L10n.text("双线性（线性光）")).tag("bilinear");Text(L10n.text("最近邻")).tag("nearest")}
            Text(L10n.text("任意角度旋转会扩展画板并保留图层结构。描边与光晕按原像素尺寸重新渲染。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Button(L10n.text("应用整图角度")){model.send(.command("image.rotate",["angle_degrees":arbitraryAngle,"interpolation":interpolation]))}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!arbitraryAngle.isFinite||abs(arbitraryAngle)>360000)
            Button(L10n.text("裁去透明边…")){showTrim=true}.buttonStyle(WorkspaceButtonStyle()).popover(isPresented:$showTrim){TransparentTrimControls(model:model,presented:$showTrim)}
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
            Text(L10n.text("裁去透明边")).fontWeight(.medium)
            Text(L10n.text("按可见合成的透明边界裁切画板，保留原始图层与像素。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{Toggle(L10n.text("顶部"),isOn:$top);Toggle(L10n.text("底部"),isOn:$bottom)}
            HStack{Toggle(L10n.text("左边"),isOn:$left);Toggle(L10n.text("右边"),isOn:$right)}
            Button(L10n.text("应用透明边裁切")){model.send(.command("image.trim",["top":top,"bottom":bottom,"left":left,"right":right]));presented=false}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!top && !bottom && !left && !right)
        }.padding(14).frame(width:240).disabled(!model.permissions.edits)
    }
}
