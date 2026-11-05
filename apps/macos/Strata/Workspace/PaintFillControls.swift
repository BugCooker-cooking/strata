import SwiftUI

struct PaintFillOptions:View{
    @ObservedObject var model:WorkspaceModel
    private func update<T>(_ key:ReferenceWritableKeyPath<WorkspaceModel,T>)->Binding<T>{Binding(get:{model[keyPath:key]},set:{model[keyPath:key]=$0;model.send(.tool(model.tool))})}
    var body:some View{
        HStack(spacing:10){
            if model.tool == .gradient{
                Picker("渐变形状",selection:update(\.gradientShape)){Text("线性").tag("linear");Text("径向").tag("radial");Text("角度").tag("angle");Text("对称").tag("reflected");Text("菱形").tag("diamond")}.frame(width:100)
                Toggle("反向",isOn:update(\.gradientReverse));opacity
                Text("拖动设置起终点 · 前景色到背景色").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }else{
                Picker("取样",selection:update(\.paintSample)){Text("可见合成").tag("merged");Text("当前像素层").tag("current_layer")}.frame(width:120)
                if model.tool == .bucket{
                    Text("容差");TextField("油漆桶容差",value:update(\.bucketTolerance),format:.number.precision(.fractionLength(0))).frame(width:45)
                    Toggle("连续",isOn:update(\.bucketContiguous));opacity
                }else{
                    Text("取样半径");TextField("吸管取样半径",value:update(\.sampleRadius),format:.number.precision(.fractionLength(0))).frame(width:45);Text("px · ⌥ 设置背景色")
                    Text(model.sampledColorSummary).foregroundStyle(Color(nsColor:WorkspaceColors.secondary)).lineLimit(1)
                }
            }
            Menu("填充"){
                Button("前景色填充"){fill(model.foregroundColor)}
                Button("背景色填充"){fill(model.backgroundColor)}
            }.disabled(!model.permissions.edits || model.selected?.kind?.kind != "pixel")
        }.labelsHidden().textFieldStyle(.roundedBorder).disabled(model.busy)
    }
    private var opacity:some View{HStack(spacing:4){Text("不透明度");TextField("填充不透明度",value:update(\.paintOpacity),format:.number.precision(.fractionLength(1))).frame(width:45);Text("%")}}
    private func fill(_ value:NSColor){guard let layer=model.selected,let color=value.usingColorSpace(model.workingColorSpace)else{return};let rgba=[color.redComponent,color.greenComponent,color.blueComponent,color.alphaComponent];model.send(.command("paint.fill",["layer":["id":layer.id],"color":rgba,"opacity":min(1,max(0,model.paintOpacity/100))]))}
}
