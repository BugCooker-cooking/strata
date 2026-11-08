import SwiftUI

struct GuidePanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var name="参考线"
    @State private var axis="vertical"
    @State private var position=0.0
    @State private var selected:String?
    @State private var originX=0.0
    @State private var originY=0.0
    private var presentation:EditorSnapshot.Presentation{model.snapshot?.presentation ?? .defaults}
    private var canEdit:Bool{model.permissions.edits && model.hasDocument && !presentation.guidesLocked}
    var body:some View{
        ScrollView{VStack(alignment:.leading,spacing:10){
            Text("标尺与参考线").fontWeight(.medium)
            setting("显示标尺",key:"rulers_visible",value:presentation.rulersVisible)
            Picker("标尺单位",selection:Binding(get:{presentation.rulerUnit ?? "pixels"},set:{set("ruler_unit",$0)})){
                Text("像素 px").tag("pixels");Text("英寸 in").tag("inches");Text("厘米 cm").tag("centimeters");Text("毫米 mm").tag("millimeters");Text("点 pt").tag("points");Text("百分比 %").tag("percent")
            }.disabled(model.busy)
            Text("标尺原点（画布 px）").font(.system(size:11))
            HStack{TextField("原点 X",value:$originX,format:.number).textFieldStyle(.roundedBorder).accessibilityLabel("标尺原点 X");TextField("原点 Y",value:$originY,format:.number).textFieldStyle(.roundedBorder).accessibilityLabel("标尺原点 Y")}
            HStack{Button("应用原点"){set("ruler_origin",[originX,originY])}.disabled(model.busy || !originValid);Button("重置原点"){set("ruler_origin",[0.0,0.0])}.disabled(model.busy)}.buttonStyle(WorkspaceButtonStyle())
            Text("物理单位按文档 DPI 换算；百分比各轴对应画板宽高。参考线输入仍使用画布 px。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            setting("显示参考线",key:"guides_visible",value:presentation.guidesVisible)
            setting("锁定参考线",key:"guides_locked",value:presentation.guidesLocked)
            setting("移动吸附",key:"snap_enabled",value:presentation.snapEnabled)
            setting("吸附画板边缘",key:"snap_artboard",value:presentation.snapArtboard)
            setting("智能参考线 · 图层边缘与中心",key:"snap_layers",value:presentation.snapLayers ?? false)
            HStack{Text("吸附距离");TextField("屏幕点",value:Binding<Double>(get:{presentation.snapTolerance},set:{set("snap_tolerance",$0)}),format:.number).textFieldStyle(.roundedBorder);Text("pt")}.disabled(model.busy)
            Divider()
            TextField("参考线名称",text:$name).textFieldStyle(.roundedBorder)
            Picker("方向",selection:$axis){Text("垂直 X").tag("vertical");Text("水平 Y").tag("horizontal")}
            HStack{Text("画布坐标");TextField("参考线位置",value:$position,format:.number.precision(.fractionLength(3))).textFieldStyle(.roundedBorder);Text("px")}
            HStack{
                Button(selected==nil ? "新建参考线":"应用参考线"){commit()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!canEdit || !valid)
                Button("新建"){selected=nil}.buttonStyle(WorkspaceButtonStyle())
            }
            if let selected{Button("删除所选参考线"){model.send(.command("view.guides.delete",["guide":["id":selected]]));self.selected=nil}.buttonStyle(WorkspaceButtonStyle()).disabled(!canEdit)}
            Divider()
            Text("参考线 \(model.snapshot?.guides?.count ?? 0)")
            LazyVStack(alignment:.leading,spacing:4){ForEach(model.snapshot?.guides ?? [],id:\.id){guide in
                Button{selected=guide.id;name=guide.name;axis=guide.axis;position=guide.position}label:{Text("\(guide.axis=="vertical" ? "X":"Y") \(guide.position.formatted(.number.precision(.fractionLength(3)))) · \(guide.name)").lineLimit(1).frame(maxWidth:.infinity,alignment:.leading)}.buttonStyle(WorkspaceButtonStyle(selected:selected==guide.id))
            }}
            Button("清除全部参考线"){model.send(.command("view.guides.clear",[:]))}.buttonStyle(WorkspaceButtonStyle()).disabled(!canEdit || model.snapshot?.guides?.isEmpty != false)
            Text("参考线使用画布绝对坐标，图像旋转或缩放后仍留在原处，不导出到图片。可从标尺拖入；V工具拖动现有线，Esc取消。图层位置按整数像素吸附；小数参考线可能留有半像素差值。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            if let input=model.guideInput,!input.error.isEmpty{Text(input.error).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
            if let input=model.snapInput,!input.error.isEmpty{Text(input.error).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
        }.padding(12)}.onChange(of:model.snapshot?.documentId){_,_ in selected=nil}
            .onAppear{syncOrigin()}.onChange(of:presentation){_,_ in syncOrigin()}
            .onChange(of:model.snapshot?.guides){_,guides in if let selected,let guide=guides?.first(where:{$0.id==selected}){name=guide.name;axis=guide.axis;position=guide.position}else if selected != nil{selected=nil}}
    }
    private var valid:Bool{name.utf8.count<=256 && !name.unicodeScalars.contains(where:{CharacterSet.controlCharacters.contains($0)}) && position.isFinite && abs(position)<=1e12}
    private var originValid:Bool{[originX,originY].allSatisfy{$0.isFinite&&abs($0)<=1e12}}
    private func syncOrigin(){let values=presentation.rulerOrigin ?? [0,0];guard values.count==2 else{return};originX=values[0];originY=values[1]}
    private func set(_ key:String,_ value:Any){model.send(.command("view.presentation.set",[key:value,"expected_version":presentation.version]))}
    private func setting(_ title:String,key:String,value:Bool)->some View{Toggle(title,isOn:Binding(get:{value},set:{set(key,$0)})).disabled(model.busy)}
    private func commit(){if let selected{model.send(.command("view.guides.update",["guide":["id":selected],"name":name,"axis":axis,"position":position]))}else{model.send(.command("view.guides.add",["name":name,"axis":axis,"position":position]))}}
}
