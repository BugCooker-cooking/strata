import SwiftUI

struct GeneratedLayerControls:View{
    @ObservedObject var model:WorkspaceModel
    @State private var start=Color.white
    @State private var end=Color.black
    @State private var angle=0.0
    @State private var scale=1.0
    @State private var radial=false
    @State private var hue=0.0
    @State private var saturation=0.0
    @State private var lightness=0.0
    var body:some View{
        VStack(alignment:.leading,spacing:9){
            if model.selected?.kind?.kind=="hue_saturation"{
                HStack{Text(L10n.text("色相"));TextField(L10n.text("色相"),value:$hue,format:.number).textFieldStyle(.roundedBorder);Text("°")}
                HStack{Text(L10n.text("饱和度"));TextField(L10n.text("饱和度"),value:$saturation,format:.number).textFieldStyle(.roundedBorder)}
                HStack{Text(L10n.text("明度"));TextField(L10n.text("明度"),value:$lightness,format:.number).textFieldStyle(.roundedBorder)}
            }else{
                ColorPicker(model.selected?.kind?.kind=="solid" ? L10n.text("填充颜色") : L10n.text("起点颜色"),selection:$start,supportsOpacity:true)
                if model.selected?.kind?.kind=="gradient"{
                    ColorPicker(L10n.text("终点颜色"),selection:$end,supportsOpacity:true)
                    HStack{Text(L10n.text("角度"));TextField(L10n.text("角度"),value:$angle,format:.number).textFieldStyle(.roundedBorder)}
                    HStack{Text(L10n.text("缩放"));TextField(L10n.text("缩放"),value:$scale,format:.number).textFieldStyle(.roundedBorder)}
                    Toggle(L10n.text("径向渐变"),isOn:$radial)
                }
            }
            Button(L10n.text("应用参数")){apply()}.buttonStyle(.bordered)
        }.disabled(!model.permissions.edits || model.selected?.locks?.all == true)
            .onAppear(perform:sync).onChange(of:model.selected?.id){_,_ in sync()}.onChange(of:model.snapshot?.revision){_,_ in sync()}
    }
    private var maximum:Double{model.snapshot?.bitDepth==16 ? 65535 : 255}
    private var space:NSColorSpace{model.workingColorSpace}
    private func color(_ values:[UInt16]?)->Color{
        guard let values,values.count==4 else{return .white}
        return Color(nsColor:NSColor(colorSpace:space,components:values.map{CGFloat(Double($0)/maximum)},count:4))
    }
    private func rgba(_ color:Color)->[Int]{
        let value=NSColor(color).usingColorSpace(space) ?? .white
        return [value.redComponent,value.greenComponent,value.blueComponent,value.alphaComponent].map{Int((Double($0)*maximum).rounded().clamped(to:0...maximum))}
    }
    private func sync(){
        guard let kind=model.selected?.kind else{return}
        start=color(kind.color ?? kind.start);end=color(kind.end);angle=kind.angle ?? 0;scale=kind.scale ?? 1;radial=kind.radial ?? false
        hue=kind.hue ?? 0;saturation=kind.saturation ?? 0;lightness=kind.lightness ?? 0
    }
    private func apply(){
        guard let layer=model.selected else{return}
        let target=["id":layer.id]
        switch layer.kind?.kind{
        case "solid":model.send(.command("layer.fill.solid.set",["layer":target,"color":rgba(start)]))
        case "gradient":model.send(.command("layer.fill.gradient.set",["layer":target,"start":rgba(start),"end":rgba(end),"angle":angle,"scale":scale,"radial":radial]))
        default:model.send(.command("adjust.hue_saturation.set",["layer":target,"hue":hue,"saturation":saturation,"lightness":lightness]))
        }
    }
}
private extension Double{
    func clamped(to range:ClosedRange<Double>)->Double{min(range.upperBound,max(range.lowerBound,self))}
}
