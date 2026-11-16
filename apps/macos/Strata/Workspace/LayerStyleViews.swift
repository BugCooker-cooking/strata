import SwiftUI

struct LayerStyleControls:View{
    @ObservedObject var model:WorkspaceModel
    @State private var shadow=false
    @State private var stroke=false
    @State private var glow=false
    @State private var overlay=false
    @State private var gradient=false
    @State private var shadowColor=Color.black.opacity(0.5)
    @State private var strokeColor=Color.white
    @State private var glowColor=Color.white
    @State private var overlayColor=Color.white
    @State private var start=Color.black
    @State private var end=Color.white
    @State private var dx=4.0
    @State private var dy=4.0
    @State private var blur=3.0
    @State private var strokeSize=2.0
    @State private var strokePosition="outside"
    @State private var glowSize=8.0
    @State private var opacity=1.0
    @State private var gradientOpacity=1.0
    @State private var angle=0.0
    @State private var overlayBlend="normal"
    @State private var gradientBlend="normal"
    @State private var gradientScale=1.0
    @State private var radial=false
    var body:some View{
        DisclosureGroup(L10n.text("图层样式")){
            VStack(alignment:.leading,spacing:10){
                Toggle(L10n.text("投影"),isOn:$shadow)
                if shadow{ColorPicker(L10n.text("投影颜色"),selection:$shadowColor);number(L10n.text("水平位移"),value:$dx);number(L10n.text("垂直位移"),value:$dy);number(L10n.text("模糊"),value:$blur)}
                Toggle(L10n.text("描边"),isOn:$stroke)
                if stroke{ColorPicker(L10n.text("描边颜色"),selection:$strokeColor);number(L10n.text("描边大小"),value:$strokeSize)
                    Picker(L10n.text("位置"),selection:$strokePosition){Text(L10n.text("外部")).tag("outside");Text(L10n.text("内部")).tag("inside");Text(L10n.text("居中")).tag("center")}}
                Toggle(L10n.text("外发光"),isOn:$glow)
                if glow{ColorPicker(L10n.text("发光颜色"),selection:$glowColor);number(L10n.text("发光范围"),value:$glowSize)}
                Toggle(L10n.text("颜色叠加"),isOn:$overlay)
                if overlay{ColorPicker(L10n.text("叠加颜色"),selection:$overlayColor);number(L10n.text("不透明度（0–1）"),value:$opacity);blend(L10n.text("混合模式"),value:$overlayBlend)}
                Toggle(L10n.text("渐变叠加"),isOn:$gradient)
                if gradient{ColorPicker(L10n.text("渐变起点"),selection:$start);ColorPicker(L10n.text("渐变终点"),selection:$end);number(L10n.text("角度"),value:$angle);number(L10n.text("缩放"),value:$gradientScale);Toggle(L10n.text("径向"),isOn:$radial);number(L10n.text("不透明度（0–1）"),value:$gradientOpacity);blend(L10n.text("混合模式"),value:$gradientBlend)}
                Button(L10n.text("应用样式")){apply()}
            }.padding(.top,8)
        }.disabled(!model.permissions.edits || model.selected?.locks?.all == true)
            .onAppear(perform:sync).onChange(of:model.selected?.id){_,_ in sync()}.onChange(of:model.snapshot?.revision){_,_ in sync()}
    }
    private var maximum:Double{model.snapshot?.bitDepth==16 ? 65535 : 255}
    private var space:NSColorSpace{model.workingColorSpace}
    private func number(_ title:String,value:Binding<Double>)->some View{HStack{Text(title).font(.system(size:10));Spacer();TextField(title,value:value,format:.number).textFieldStyle(.roundedBorder).frame(width:70)}}
    private func blend(_ title:String,value:Binding<String>)->some View{Picker(title,selection:value){ForEach(LayerBlendChoice.all,id:\.id){Text($0.title).tag($0.id)}}}
    private func color(_ value:AIJSON,fallback:Color)->Color{
        let values=value.array.compactMap(\.number);guard values.count==4 else{return fallback}
        return Color(nsColor:NSColor(colorSpace:space,components:values.map{CGFloat($0/maximum)},count:4))
    }
    private func rgba(_ value:Color)->[Int]{
        let c=NSColor(value).usingColorSpace(space) ?? .white
        return [c.redComponent,c.greenComponent,c.blueComponent,c.alphaComponent].map{Int(min(maximum,max(0,Double($0)*maximum)).rounded())}
    }
    private func sync(){
        let styles=model.selected?.styles ?? .null
        shadow=styles["drop_shadow"] != .null;stroke=styles["stroke"] != .null;glow=styles["outer_glow"] != .null;overlay=styles["color_overlay"] != .null;gradient=styles["gradient_overlay"] != .null
        shadowColor=color(styles["drop_shadow"]["color"],fallback:.black.opacity(0.5));strokeColor=color(styles["stroke"]["color"],fallback:.white);glowColor=color(styles["outer_glow"]["color"],fallback:.white);overlayColor=color(styles["color_overlay"]["color"],fallback:.white)
        dx=styles["drop_shadow"]["offset_x"].number ?? 4;dy=styles["drop_shadow"]["offset_y"].number ?? 4;blur=styles["drop_shadow"]["blur"].number ?? 3
        strokeSize=styles["stroke"]["size"].number ?? 2;strokePosition=styles["stroke"]["position"].string ?? "outside";glowSize=styles["outer_glow"]["size"].number ?? 8
        opacity=styles["color_overlay"]["opacity"].number ?? 1;gradientOpacity=styles["gradient_overlay"]["opacity"].number ?? 1
        overlayBlend=styles["color_overlay"]["blend_mode"].string ?? "normal";gradientBlend=styles["gradient_overlay"]["blend_mode"].string ?? "normal";gradientScale=styles["gradient_overlay"]["scale"].number ?? 1;radial=styles["gradient_overlay"]["radial"].bool
        start=color(styles["gradient_overlay"]["start"],fallback:.black);end=color(styles["gradient_overlay"]["end"],fallback:.white);angle=styles["gradient_overlay"]["angle"].number ?? 0
    }
    private func apply(){
        guard let layer=model.selected,let ix=Int32(exactly:dx),let iy=Int32(exactly:dy) else{return}
        var styles:[String:Any]=[:]
        if shadow{styles["drop_shadow"]=["color":rgba(shadowColor),"offset_x":ix,"offset_y":iy,"blur":blur]}
        if stroke{styles["stroke"]=["color":rgba(strokeColor),"size":strokeSize,"position":strokePosition]}
        if glow{styles["outer_glow"]=["color":rgba(glowColor),"size":glowSize]}
        if overlay{styles["color_overlay"]=["color":rgba(overlayColor),"opacity":opacity,"blend_mode":overlayBlend]}
        if gradient{styles["gradient_overlay"]=["start":rgba(start),"end":rgba(end),"angle":angle,"scale":gradientScale,"opacity":gradientOpacity,"radial":radial,"blend_mode":gradientBlend]}
        model.send(.command("layer.set_styles",["layer":["id":layer.id],"styles":styles]))
    }
}
