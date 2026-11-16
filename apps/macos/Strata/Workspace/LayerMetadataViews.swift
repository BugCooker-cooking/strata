import SwiftUI

struct LayerBlendChoice {
    let id:String;let title:String
    static let all:[Self]=[
        .init(id:"normal",title:L10n.text("正常")),.init(id:"dissolve",title:L10n.text("溶解")),.init(id:"darken",title:L10n.text("变暗")),
        .init(id:"multiply",title:L10n.text("正片叠底")),.init(id:"color_burn",title:L10n.text("颜色加深")),.init(id:"linear_burn",title:L10n.text("线性加深")),
        .init(id:"darker_color",title:L10n.text("较深色")),.init(id:"lighten",title:L10n.text("变亮")),.init(id:"screen",title:L10n.text("滤色")),
        .init(id:"color_dodge",title:L10n.text("颜色减淡")),.init(id:"linear_dodge",title:L10n.text("线性减淡")),.init(id:"lighter_color",title:L10n.text("较浅色")),
        .init(id:"overlay",title:L10n.text("叠加")),.init(id:"soft_light",title:L10n.text("柔光")),.init(id:"hard_light",title:L10n.text("强光")),
        .init(id:"vivid_light",title:L10n.text("亮光")),.init(id:"linear_light",title:L10n.text("线性光")),.init(id:"pin_light",title:L10n.text("点光")),
        .init(id:"hard_mix",title:L10n.text("实色混合")),.init(id:"difference",title:L10n.text("差值")),.init(id:"exclusion",title:L10n.text("排除")),
        .init(id:"subtract",title:L10n.text("减去")),.init(id:"divide",title:L10n.text("划分")),.init(id:"hue",title:L10n.text("色相")),
        .init(id:"saturation",title:L10n.text("饱和度")),.init(id:"color",title:L10n.text("颜色")),.init(id:"luminosity",title:L10n.text("明度"))]
}
struct LayerMetadataControls:View {
    @ObservedObject var model:WorkspaceModel
    @State private var fill=100.0
    static func labelColor(_ value:String)->Color{
        switch value{case "red":.red;case "orange":.orange;case "yellow":.yellow;case "green":.green;case "blue":.blue;case "violet":.purple;default:.gray}
    }
    var body:some View {
        if let layer=model.selected{
            VStack(alignment:.leading,spacing:8){
                if layer.isGroup{Toggle(L10n.text("穿透到下方图层"),isOn:Binding(get:{layer.passThrough ?? false},set:{model.send(.command("layer.set_pass_through",["layer":["id":layer.id],"pass_through":$0]))})).disabled(layer.locks?.all == true)}
                HStack{
                    Text(L10n.text("填充")).font(.system(size:11)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
                    Slider(value:$fill,in:0...100,onEditingChanged:{editing in if !editing{model.send(.command("layer.set_fill_opacity",["layer":["id":layer.id],"fill_opacity":fill/100]))}})
                        .disabled(layer.locks?.all == true).accessibilityLabel(L10n.text("填充不透明度"))
                    Text("\(Int(fill))%").font(.system(size:10)).frame(width:32)
                }
                HStack(spacing:7){
                    Text(L10n.text("锁定")).font(.system(size:11)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
                    lock(L10n.text("透明像素"),symbol:"checkerboard.rectangle",key:"transparency",value:layer.locks?.transparency ?? false)
                    lock(L10n.text("像素"),symbol:"paintbrush",key:"pixels",value:layer.locks?.pixels ?? false)
                    lock(L10n.text("位置"),symbol:"arrow.up.and.down.and.arrow.left.and.right",key:"position",value:layer.locks?.position ?? false)
                    lock(L10n.text("全部"),symbol:"lock",key:"all",value:layer.locks?.all ?? false)
                }
                Picker(L10n.text("颜色标签"),selection:Binding(get:{layer.colorLabel ?? ""},set:{value in
                    model.send(.command("layer.set_color_label",["layer":["id":layer.id],"color_label":value.isEmpty ? NSNull() : value as Any]))
                })){
                    Text(L10n.text("无")).tag("")
                    ForEach([(L10n.text("红"),"red"),(L10n.text("橙"),"orange"),(L10n.text("黄"),"yellow"),(L10n.text("绿"),"green"),(L10n.text("蓝"),"blue"),(L10n.text("紫"),"violet"),(L10n.text("灰"),"gray")],id:\.1){title,id in Text(title).tag(id)}
                }.disabled(layer.locks?.all == true)
            }.disabled(!model.permissions.edits)
                .onAppear{fill=(layer.fillOpacity ?? 1)*100}
                .onChange(of:layer.id){_,_ in fill=(model.selected?.fillOpacity ?? 1)*100}
                .onChange(of:layer.fillOpacity){_,value in fill=(value ?? 1)*100}
        }
    }
    private func lock(_ title:String,symbol:String,key:String,value:Bool)->some View{
        WorkspaceIconButton(title:L10n.format("锁定{0}", title),symbol:symbol,selected:value){
            guard let layer=model.selected else{return}
            var locks:[String:Bool]=["transparency":layer.locks?.transparency ?? false,"pixels":layer.locks?.pixels ?? false,"position":layer.locks?.position ?? false,"all":layer.locks?.all ?? false]
            locks[key] = !value
            model.send(.command("layer.set_locks",["layer":["id":layer.id],"locks":locks]))
        }
    }
}
