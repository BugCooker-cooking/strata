import SwiftUI

struct BrushToolOptions: View {
    @ObservedObject var model: WorkspaceModel
    private func update<T>(_ key: ReferenceWritableKeyPath<WorkspaceModel,T>) -> Binding<T> {
        Binding(get:{model[keyPath:key]},set:{model[keyPath:key]=$0;model.send(.tool(model.tool))})
    }
    var body: some View {
        HStack(spacing:8) {
            if model.tool == .clone{
                Picker("仿制取样",selection:update(\.cloneSample)){Text("当前层").tag("current_layer");Text("可见合成").tag("merged")}.frame(width:110).labelsHidden()
                Toggle("对齐",isOn:update(\.cloneAligned))
            }else{Picker("绘制目标",selection:update(\.brushTarget)) {Text("像素").tag("pixels");Text("图层蒙版").tag("mask")}.frame(width:110).labelsHidden()}
            field("大小",update(\.brushSize),unit:"px")
            field("硬度",update(\.brushHardness),unit:"%")
            field("不透明度",update(\.brushOpacity),unit:"%")
            field("流量",update(\.brushFlow),unit:"%")
            field("间距",update(\.brushSpacing),unit:"%")
            Toggle("压感大小",isOn:update(\.brushPressureSize))
            Toggle("压感不透明度",isOn:update(\.brushPressureOpacity))
            Text(model.tool == .clone ? "⌥ 设置源点 · Esc 取消笔触" : "Esc 取消当前笔触").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
        }.textFieldStyle(.roundedBorder).disabled(!model.permissions.edits)
    }
    private func field(_ title:String,_ value:Binding<Double>,unit:String) -> some View {
        HStack(spacing:3){Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(1))).frame(width:44).accessibilityLabel("画笔"+title);Text(unit).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
    }
}
struct PaintColorControls: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        VStack(spacing:4) {
            ColorPicker("前景色",selection:binding(\.foregroundColor),supportsOpacity:false).labelsHidden().help("前景色")
            ColorPicker("背景色",selection:binding(\.backgroundColor),supportsOpacity:false).labelsHidden().help("背景色")
            WorkspaceIconButton(title:"交换前景色与背景色 (X)",symbol:"arrow.up.arrow.down") {model.swapPaintColors()}
        }.disabled(model.busy)
    }
    private func binding(_ key:ReferenceWritableKeyPath<WorkspaceModel,NSColor>) -> Binding<Color> {
        Binding(get:{Color(nsColor:model[keyPath:key])},set:{color in
            model[keyPath:key]=NSColor(color).usingColorSpace(model.workingColorSpace) ?? NSColor(color)
            model.send(.tool(model.tool))
        })
    }
}
