import SwiftUI

struct BrushToolOptions: View {
    @ObservedObject var model: WorkspaceModel
    private func update<T>(_ key: ReferenceWritableKeyPath<WorkspaceModel,T>) -> Binding<T> {
        Binding(get:{model[keyPath:key]},set:{model[keyPath:key]=$0;model.send(.tool(model.tool))})
    }
    var body: some View {
        ScrollView(.horizontal,showsIndicators:false){HStack(spacing:8) {
            if model.tool == .dodge {
                Picker("修饰模式",selection:update(\.tonalBrushMode)){Text("减淡").tag("dodge");Text("加深").tag("burn");Text("海绵").tag("sponge")}.frame(width:100).labelsHidden()
                if model.tonalBrushMode == "sponge" {
                    Picker("海绵模式",selection:update(\.spongeMode)){Text("降低饱和度").tag("desaturate");Text("增加饱和度").tag("saturate")}.frame(width:130).labelsHidden()
                    Toggle("自然饱和度",isOn:update(\.spongeVibrance))
                }else{
                    Picker("色调范围",selection:update(\.tonalBrushRange)){Text("阴影").tag("shadows");Text("中间调").tag("midtones");Text("高光").tag("highlights")}.frame(width:100).labelsHidden()
                    Toggle("保护色调",isOn:update(\.tonalBrushProtect))
                }
                field(model.tonalBrushMode == "sponge" ? "强度" : "曝光量",update(\.tonalBrushExposure),unit:"%")
            }
            if model.tool == .heal {
                Picker("修复模式",selection:update(\.healMode)){Text("取样修复").tag("sampled");Text("污点修复").tag("spot")}.frame(width:110).labelsHidden()
            }
            if [.clone,.heal].contains(model.tool){
                Picker("取样范围",selection:update(\.cloneSample)){Text("当前层").tag("current_layer");Text("可见合成").tag("merged")}.frame(width:110).labelsHidden()
                if model.tool == .heal && model.healMode == "spot" {field("搜索范围",update(\.spotSearchRadius),unit:"px")}
                else {Toggle("对齐",isOn:update(\.cloneAligned))}
            }else if model.tool != .dodge{Picker("绘制目标",selection:update(\.brushTarget)) {Text("像素").tag("pixels");Text("图层蒙版").tag("mask")}.frame(width:110).labelsHidden()}
            field("大小",update(\.brushSize),unit:"px")
            field("硬度",update(\.brushHardness),unit:"%")
            field("不透明度",update(\.brushOpacity),unit:"%")
            field("流量",update(\.brushFlow),unit:"%")
            field("间距",update(\.brushSpacing),unit:"%")
            Toggle("压感大小",isOn:update(\.brushPressureSize))
            Toggle("压感不透明度",isOn:update(\.brushPressureOpacity))
            Text(model.tool == .heal && model.healMode == "spot" ? "自动近邻匹配 · Esc 取消笔触" : [.clone,.heal].contains(model.tool) ? "⌥ 设置源点 · Esc 取消笔触" : "Esc 取消当前笔触").foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
        }.fixedSize(horizontal:true,vertical:false)}.textFieldStyle(.roundedBorder).disabled(!model.permissions.edits)
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
