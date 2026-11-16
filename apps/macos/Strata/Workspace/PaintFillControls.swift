import SwiftUI

struct PaintFillOptions:View{
    @ObservedObject var model:WorkspaceModel
    private func update<T>(_ key:ReferenceWritableKeyPath<WorkspaceModel,T>)->Binding<T>{Binding(get:{model[keyPath:key]},set:{model[keyPath:key]=$0;model.send(.tool(model.tool))})}
    var body:some View{
        HStack(spacing:10){
            if model.tool == .gradient{
                Picker(L10n.text("渐变形状"),selection:update(\.gradientShape)){Text(L10n.text("线性")).tag("linear");Text(L10n.text("径向")).tag("radial");Text(L10n.text("角度")).tag("angle");Text(L10n.text("对称")).tag("reflected");Text(L10n.text("菱形")).tag("diamond")}.frame(width:100)
                Toggle(L10n.text("反向"),isOn:update(\.gradientReverse));opacity
                Text(L10n.text("拖动设置起终点 · 前景色到背景色")).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }else{
                Picker(L10n.text("取样"),selection:update(\.paintSample)){Text(L10n.text("可见合成")).tag("merged");Text(L10n.text("当前像素层")).tag("current_layer")}.frame(width:120)
                if model.tool == .bucket{
                    Text(L10n.text("容差"));TextField(L10n.text("油漆桶容差"),value:update(\.bucketTolerance),format:.number.precision(.fractionLength(0))).frame(width:45)
                    Toggle(L10n.text("连续"),isOn:update(\.bucketContiguous));opacity
                }else{
                    Text(L10n.text("取样半径"));TextField(L10n.text("吸管取样半径"),value:update(\.sampleRadius),format:.number.precision(.fractionLength(0))).frame(width:45);Text(L10n.text("px · ⌥ 设置背景色"))
                    Text(model.sampledColorSummary).foregroundStyle(Color(nsColor:WorkspaceColors.secondary)).lineLimit(1)
                }
            }
            Menu(L10n.text("填充")){
                Button(L10n.text("前景色填充")){fill(model.foregroundColor)}
                Button(L10n.text("背景色填充")){fill(model.backgroundColor)}
            }.disabled(!model.permissions.edits || model.selected?.kind?.kind != "pixel")
        }.labelsHidden().textFieldStyle(.roundedBorder).disabled(model.busy)
    }
    private var opacity:some View{HStack(spacing:4){Text(L10n.text("不透明度"));TextField(L10n.text("填充不透明度"),value:update(\.paintOpacity),format:.number.precision(.fractionLength(1))).frame(width:45);Text("%")}}
    private func fill(_ value:NSColor){guard let layer=model.selected,let color=value.usingColorSpace(model.workingColorSpace)else{return};let rgba=[color.redComponent,color.greenComponent,color.blueComponent,color.alphaComponent];model.send(.command("paint.fill",["layer":["id":layer.id],"color":rgba,"opacity":min(1,max(0,model.paintOpacity/100))]))}
}
