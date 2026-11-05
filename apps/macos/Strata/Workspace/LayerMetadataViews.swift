import SwiftUI

struct LayerBlendChoice {
    let id:String;let title:String
    static let all:[Self]=[
        .init(id:"normal",title:"正常"),.init(id:"dissolve",title:"溶解"),.init(id:"darken",title:"变暗"),
        .init(id:"multiply",title:"正片叠底"),.init(id:"color_burn",title:"颜色加深"),.init(id:"linear_burn",title:"线性加深"),
        .init(id:"darker_color",title:"深色"),.init(id:"lighten",title:"变亮"),.init(id:"screen",title:"滤色"),
        .init(id:"color_dodge",title:"颜色减淡"),.init(id:"linear_dodge",title:"线性减淡"),.init(id:"lighter_color",title:"浅色"),
        .init(id:"overlay",title:"叠加"),.init(id:"soft_light",title:"柔光"),.init(id:"hard_light",title:"强光"),
        .init(id:"vivid_light",title:"亮光"),.init(id:"linear_light",title:"线性光"),.init(id:"pin_light",title:"点光"),
        .init(id:"hard_mix",title:"实色混合"),.init(id:"difference",title:"差值"),.init(id:"exclusion",title:"排除"),
        .init(id:"subtract",title:"减去"),.init(id:"divide",title:"划分"),.init(id:"hue",title:"色相"),
        .init(id:"saturation",title:"饱和度"),.init(id:"color",title:"颜色"),.init(id:"luminosity",title:"明度")]
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
                if layer.isGroup{Toggle("穿透到下方图层",isOn:Binding(get:{layer.passThrough ?? false},set:{model.send(.command("layer.set_pass_through",["layer":["id":layer.id],"pass_through":$0]))})).disabled(layer.locks?.all == true)}
                HStack{
                    Text("填充").font(.system(size:11)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
                    Slider(value:$fill,in:0...100,onEditingChanged:{editing in if !editing{model.send(.command("layer.set_fill_opacity",["layer":["id":layer.id],"fill_opacity":fill/100]))}})
                        .disabled(layer.locks?.all == true).accessibilityLabel("填充不透明度")
                    Text("\(Int(fill))%").font(.system(size:10)).frame(width:32)
                }
                HStack(spacing:7){
                    Text("锁定").font(.system(size:11)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
                    lock("透明像素",symbol:"checkerboard.rectangle",key:"transparency",value:layer.locks?.transparency ?? false)
                    lock("像素",symbol:"paintbrush",key:"pixels",value:layer.locks?.pixels ?? false)
                    lock("位置",symbol:"arrow.up.and.down.and.arrow.left.and.right",key:"position",value:layer.locks?.position ?? false)
                    lock("全部",symbol:"lock",key:"all",value:layer.locks?.all ?? false)
                }
                Picker("颜色标签",selection:Binding(get:{layer.colorLabel ?? ""},set:{value in
                    model.send(.command("layer.set_color_label",["layer":["id":layer.id],"color_label":value.isEmpty ? NSNull() : value as Any]))
                })){
                    Text("无").tag("")
                    ForEach([("红","red"),("橙","orange"),("黄","yellow"),("绿","green"),("蓝","blue"),("紫","violet"),("灰","gray")],id:\.1){title,id in Text(title).tag(id)}
                }.disabled(layer.locks?.all == true)
            }.disabled(!model.permissions.edits)
                .onAppear{fill=(layer.fillOpacity ?? 1)*100}
                .onChange(of:layer.id){_,_ in fill=(model.selected?.fillOpacity ?? 1)*100}
                .onChange(of:layer.fillOpacity){_,value in fill=(value ?? 1)*100}
        }
    }
    private func lock(_ title:String,symbol:String,key:String,value:Bool)->some View{
        WorkspaceIconButton(title:"锁定"+title,symbol:symbol,selected:value){
            guard let layer=model.selected else{return}
            var locks:[String:Bool]=["transparency":layer.locks?.transparency ?? false,"pixels":layer.locks?.pixels ?? false,"position":layer.locks?.position ?? false,"all":layer.locks?.all ?? false]
            locks[key] = !value
            model.send(.command("layer.set_locks",["layer":["id":layer.id],"locks":locks]))
        }
    }
}
