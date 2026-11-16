import SwiftUI

struct BrushToolOptions: View {
    @ObservedObject var model: WorkspaceModel
    private func update<T>(_ key: ReferenceWritableKeyPath<WorkspaceModel,T>) -> Binding<T> {
        Binding(get:{model[keyPath:key]},set:{model[keyPath:key]=$0;model.send(.tool(model.tool))})
    }
    var body: some View {
        ScrollView(.horizontal,showsIndicators:false){HStack(spacing:8) {
            if model.tool == .dodge {
                Picker(L10n.text("修饰模式"),selection:update(\.tonalBrushMode)){Text(L10n.text("减淡")).tag("dodge");Text(L10n.text("加深")).tag("burn");Text(L10n.text("海绵")).tag("sponge")}.frame(width:100).labelsHidden()
                if model.tonalBrushMode == "sponge" {
                    Picker(L10n.text("海绵模式"),selection:update(\.spongeMode)){Text(L10n.text("降低饱和度")).tag("desaturate");Text(L10n.text("增加饱和度")).tag("saturate")}.frame(width:130).labelsHidden()
                    Toggle(L10n.text("自然饱和度"),isOn:update(\.spongeVibrance))
                }else{
                    Picker(L10n.text("色调范围"),selection:update(\.tonalBrushRange)){Text(L10n.text("阴影")).tag("shadows");Text(L10n.text("中间调")).tag("midtones");Text(L10n.text("高光")).tag("highlights")}.frame(width:100).labelsHidden()
                    Toggle(L10n.text("保护色调"),isOn:update(\.tonalBrushProtect))
                }
                field(model.tonalBrushMode == "sponge" ? L10n.text("强度") : L10n.text("曝光量"),update(\.tonalBrushExposure),unit:"%")
            }
            if model.tool == .heal {
                Picker(L10n.text("修复模式"),selection:update(\.healMode)){Text(L10n.text("取样修复")).tag("sampled");Text(L10n.text("污点修复")).tag("spot")}.frame(width:110).labelsHidden()
            }
            if [.clone,.heal].contains(model.tool){
                Picker(L10n.text("取样范围"),selection:update(\.cloneSample)){Text(L10n.text("当前层")).tag("current_layer");Text(L10n.text("可见合成")).tag("merged")}.frame(width:110).labelsHidden()
                if model.tool == .heal && model.healMode == "spot" {
                    Picker(L10n.text("污点修复类型"), selection: update(\.spotHealMode)) {
                        Text(L10n.text("近邻匹配")).tag("proximity_match")
                        Text(L10n.text("创建纹理")).tag("create_texture")
                        Text(L10n.text("内容识别")).tag("content_aware")
                    }.frame(width: 154).labelsHidden()
                    if model.spotHealMode != "create_texture" {
                        field(L10n.text("搜索范围"), update(\.spotSearchRadius), unit: "px")
                    }
                }
                else {Toggle(L10n.text("对齐"),isOn:update(\.cloneAligned))}
            }else if model.tool != .dodge{Picker(L10n.text("绘制目标"),selection:update(\.brushTarget)) {Text(L10n.text("像素")).tag("pixels");Text(L10n.text("图层蒙版")).tag("mask")}.frame(width:110).labelsHidden()}
            field(L10n.text("大小"),update(\.brushSize),unit:"px")
            field(L10n.text("硬度"),update(\.brushHardness),unit:"%")
            field(L10n.text("不透明度"),update(\.brushOpacity),unit:"%")
            field(L10n.text("流量"),update(\.brushFlow),unit:"%")
            field(L10n.text("间距"),update(\.brushSpacing),unit:"%")
            Toggle(L10n.text("压感大小"),isOn:update(\.brushPressureSize))
            Toggle(L10n.text("压感不透明度"),isOn:update(\.brushPressureOpacity))
            let needsSource = model.tool == .clone || model.tool == .heal && model.healMode == "sampled"
            Text(needsSource ? L10n.text("⌥ 设置源点 · Esc 取消笔触") : L10n.text("Esc 取消当前笔触"))
                .foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
        }.fixedSize(horizontal:true,vertical:false)}.textFieldStyle(.roundedBorder).disabled(!model.permissions.edits)
    }
    private func field(_ title:String,_ value:Binding<Double>,unit:String) -> some View {
        HStack(spacing:3){Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(1))).frame(width:44).accessibilityLabel(L10n.format("画笔{0}", title));Text(unit).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
    }
}
struct PaintColorControls: View {
    @ObservedObject var model: WorkspaceModel
    var body: some View {
        VStack(spacing:4) {
            ColorPicker(L10n.text("前景色"),selection:binding(\.foregroundColor),supportsOpacity:false).labelsHidden().help(L10n.text("前景色"))
            ColorPicker(L10n.text("背景色"),selection:binding(\.backgroundColor),supportsOpacity:false).labelsHidden().help(L10n.text("背景色"))
            WorkspaceIconButton(title:L10n.text("交换前景色与背景色 (X)"),symbol:"arrow.up.arrow.down") {model.swapPaintColors()}
        }.disabled(!model.permissions.edits)
    }
    private func binding(_ key:ReferenceWritableKeyPath<WorkspaceModel,NSColor>) -> Binding<Color> {
        Binding(get:{Color(nsColor:model[keyPath:key])},set:{color in
            model.setPaintColor(NSColor(color),background:key == \.backgroundColor)
        })
    }
}
