import SwiftUI

struct TransformPanel:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var preview:TonalPreviewModel
    @State private var mode="components"
    @State private var scope="whole_layer"
    @State private var interpolation="bilinear"
    @State private var x=0.0
    @State private var y=0.0
    @State private var scaleX=100.0
    @State private var scaleY=100.0
    @State private var rotation=0.0
    @State private var shearX=0.0
    @State private var shearY=0.0
    @State private var pivotX=0.0
    @State private var pivotY=0.0
    @State private var source="0,0; 100,0; 100,100; 0,100"
    @State private var destination="0,0; 100,0; 100,100; 0,100"
    static func parseCorners(_ value:String)->[[Double]]?{
        let pieces=value.split(separator:";",omittingEmptySubsequences:false)
        guard pieces.count==4 else{return nil}
        let points=pieces.compactMap{piece->[Double]? in
            let pair=piece.split(separator:",",omittingEmptySubsequences:false)
            guard pair.count==2,let x=Double(pair[0].trimmingCharacters(in:.whitespacesAndNewlines)),let y=Double(pair[1].trimmingCharacters(in:.whitespacesAndNewlines))else{return nil}
            return [x,y]
        }
        guard points.count==4,points.allSatisfy({$0.count==2&&$0.allSatisfy({$0.isFinite&&abs($0)<=1e12})})else{return nil};return points
    }
    private var parameters:[String:Any]?{
        let geometry:[String:Any]
        if mode == "corners"{
            guard let a=Self.parseCorners(source),let b=Self.parseCorners(destination)else{return nil};geometry=["type":"corners","source":a,"destination":b]
        }else{
            guard [x,y,scaleX,scaleY,rotation,shearX,shearY,pivotX,pivotY].allSatisfy({$0.isFinite}),abs(scaleX)>=0.0001,abs(scaleY)>=0.0001,abs(shearX)<89,abs(shearY)<89 else{return nil}
            geometry=["type":"components","translation":[x,y],"scale":[scaleX/100,scaleY/100],"rotation_degrees":rotation,"shear_degrees":[shearX,shearY],"pivot":[pivotX,pivotY]]
        }
        if isMultiple {
            return ["layers": model.transformLayerIDs.map { ["id": $0] }, "geometry": geometry, "interpolation": interpolation]
        }
        return ["scope":scope,"geometry":geometry,"interpolation":interpolation]
    }
    private var isMultiple: Bool { !model.transformLayerIDs.isEmpty }
    private var available: Bool { model.canTransform(scope: scope) }
    var body:some View{
        ScrollView{VStack(alignment:.leading,spacing:10){
            if model.tool == .freeTransform,let input=model.transformInput{
                CanvasTransformControls(model:model,input:input)
            }else{
            Text(L10n.text("自由变换")).fontWeight(.medium)
            Button(L10n.text("使用画布手柄（⌘T）")){model.canvasTransformScope=scope;model.canvasTransformInterpolation=interpolation;model.selectTool(.freeTransform)}.buttonStyle(WorkspaceButtonStyle()).disabled(!available)
            if isMultiple {
                Text(L10n.format("变换 {0} 个选中的图层", String(describing: model.transformLayerIDs.count))).font(.system(size: 10))
            } else {
                Picker(L10n.text("变换范围"),selection:$scope){Text(L10n.text("整个图层")).tag("whole_layer");Text(L10n.text("选区内像素")).tag("selection").disabled(model.selected?.kind?.kind != "pixel")}.disabled(preview.applying)
                Text(scope == "selection" ? L10n.text("提升选中像素后变换并合回；选区轮廓同步，图层位置与蒙版保留。") : L10n.text("当前选区保持不变；图层内容与链接蒙版一起变换，文字保留原文。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }
            Group{
            Picker(L10n.text("变换方式"),selection:$mode){Text(L10n.text("数值变换")).tag("components");Text(L10n.text("四角透视")).tag("corners")}.labelsHidden()
            if mode == "corners"{
                Text(L10n.text("按 左上、右上、右下、左下 顺序，画布坐标 x,y；用分号分隔。")).font(.system(size:10))
                TextField(L10n.text("源四角"),text:$source).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("源四角"))
                TextField(L10n.text("目标四角"),text:$destination).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("目标四角"))
                Button(L10n.text("使用当前画板四角")){artboardCorners()}.buttonStyle(WorkspaceButtonStyle())
            }else{
                pair(L10n.text("平移 X"),$x,"Y",$y)
                pair(L10n.text("缩放 X %"),$scaleX,"Y %",$scaleY)
                field(L10n.text("旋转 °"),$rotation)
                pair(L10n.text("斜切 X °"),$shearX,"Y °",$shearY)
                pair(L10n.text("轴心 X"),$pivotX,"Y",$pivotY)
                HStack{Button(L10n.text("水平翻转")){scaleX = -scaleX};Button(L10n.text("垂直翻转")){scaleY = -scaleY}}.buttonStyle(WorkspaceButtonStyle())
                Button(L10n.text("轴心设为画板中心")){if let f=model.snapshot?.frame{pivotX=Double(f.x)+Double(f.width)/2;pivotY=Double(f.y)+Double(f.height)/2}}.buttonStyle(WorkspaceButtonStyle())
            }
            Picker(L10n.text("重采样"),selection:$interpolation){Text(L10n.text("最近邻（保留整数采样）")).tag("nearest");Text(L10n.text("双线性")).tag("bilinear")}
            }.disabled(preview.applying)
            if !isMultiple {
                TonalPreviewControls(preview:preview,parameters:parameters.flatMap{try? commandJSON($0)},filterCommand:"transform.apply").disabled(!available)
            }
            HStack{
                Button(L10n.text("应用变换")){commit()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!available||parameters == nil||preview.applying)
                Button(L10n.text("取消预览")){preview.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!preview.enabled && !preview.busy && !preview.applying)
            }
            if !available{Text(L10n.text("选择未锁定的图层。调整图层需要蒙版，选区范围需要像素层。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            }
            Divider()
            ImageSizePanel(model:model)
            Divider()
            BitDepthPanel(model:model)
            Divider()
            CanvasSizePanel(model:model)
            Divider()
            CropPanel(model:model)
            Divider()
            ImageOrientationPanel(model:model)
            Divider()
            StraightenPanel(model:model)
        }.padding(12)}
    }
    private func field(_ title:String,_ value:Binding<Double>)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.format("变换{0}", title))}}
    private func pair(_ a:String,_ av:Binding<Double>,_ b:String,_ bv:Binding<Double>)->some View{HStack{field(a,av);field(b,bv)}}
    private func artboardCorners(){guard let f=model.snapshot?.frame else{return};let x=Double(f.x),y=Double(f.y),r=x+Double(f.width),b=y+Double(f.height);source="\(x),\(y); \(r),\(y); \(r),\(b); \(x),\(b)";destination=source}
    private func commit() {
        guard var parameters else { return }
        if isMultiple {
            model.send(.command("transform.apply_layers", parameters))
        } else if let layer = model.selected {
            if preview.enabled, let json = try? commandJSON(parameters) { preview.apply(json); return }
            parameters["layer"] = ["id": layer.id]
            model.send(.command("transform.apply", parameters))
        }
    }
}

private struct CanvasTransformControls:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var input:CanvasTransformInput
    private var available: Bool { model.canTransform(scope: model.canvasTransformScope, onCanvas: true) }
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text(L10n.text("画布自由变换")).fontWeight(.medium)
            if model.transformLayerIDs.isEmpty {
                Picker(L10n.text("变换范围"),selection:$model.canvasTransformScope){Text(L10n.text("整个图层")).tag("whole_layer");Text(L10n.text("选区内像素")).tag("selection").disabled(model.selected?.kind?.kind != "pixel")}.disabled(input.hasDraft||input.running)
            } else {
                Text(L10n.format("变换 {0} 个选中的图层", String(describing: model.transformLayerIDs.count))).font(.system(size: 10))
            }
            Picker(L10n.text("手柄模式"),selection:$model.canvasTransformMode){ForEach(CanvasTransformMode.allCases,id:\.self){Text($0.title).tag($0.rawValue)}}.disabled(input.running)
            Picker(L10n.text("重采样"),selection:$model.canvasTransformInterpolation){Text(L10n.text("最近邻")).tag("nearest");Text(L10n.text("双线性")).tag("bilinear")}.disabled(input.running)
            Text(L10n.text("拖动角点或边缘手柄，框内拖动平移；上方圆点旋转。Shift 约束比例、方向或 15° 旋转，Option 围绕中心缩放。透视模式用角点控制对应边。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Text(L10n.text("拖动时显示变换轮廓，图像在应用后更新。方向键微移，Shift 加速；回车应用，Esc 取消。选区模式同步变换选中像素与选区。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{
                Button(input.hasDraft ? L10n.text("重新开始"):L10n.text("读取边界")){input.start()}.buttonStyle(WorkspaceButtonStyle()).disabled(!available||input.running)
                Button(L10n.text("应用变换")){_ = input.confirm()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!input.canConfirm)
                Button(L10n.text("取消")){input.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!input.hasDraft && !input.running)
            }
            if !input.error.isEmpty{Text(input.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
            if !available && !input.running{Text(L10n.text("选择可见且未锁定的图层。调整图层需要蒙版，选区范围需要像素层。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
        }.onChange(of:model.canvasTransformMode){_,_ in model.send(.configureCanvasTransform)}
            .onChange(of:model.canvasTransformScope){_,_ in model.send(.configureCanvasTransform)}
            .onChange(of:model.canvasTransformInterpolation){_,_ in model.send(.configureCanvasTransform)}
    }
}
