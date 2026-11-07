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
        return ["scope":scope,"geometry":geometry,"interpolation":interpolation]
    }
    private var available:Bool{model.permissions.edits&&model.selected?.kind?.kind == "pixel"&&model.selected?.locks?.all != true&&model.selected?.locks?.pixels != true&&model.selected?.locks?.position != true&&model.selected?.locks?.transparency != true && (scope != "selection" || model.snapshot?.selectionExtent != nil)}
    var body:some View{
        ScrollView{VStack(alignment:.leading,spacing:10){
            if model.tool == .freeTransform,let input=model.transformInput{
                CanvasTransformControls(model:model,input:input)
            }else{
            Text("像素自由变换").fontWeight(.medium)
            Button("使用画布手柄（⌘T）"){model.canvasTransformScope=scope;model.canvasTransformInterpolation=interpolation;model.selectTool(.freeTransform)}.buttonStyle(WorkspaceButtonStyle()).disabled(!available)
            Picker("变换范围",selection:$scope){Text("整个像素图层").tag("whole_layer");Text("选区内像素").tag("selection")}.disabled(preview.applying)
            Text(scope == "selection" ? "提升选中像素后变换并合回；选区轮廓同步，图层位置与蒙版保留。" : "当前选区保持不变；整层像素与链接蒙版一起变换，保留画板外内容。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Group{
            Picker("变换方式",selection:$mode){Text("数值变换").tag("components");Text("四角透视").tag("corners")}.labelsHidden()
            if mode == "corners"{
                Text("按 左上、右上、右下、左下 顺序，画布坐标 x,y；用分号分隔。").font(.system(size:10))
                TextField("源四角",text:$source).textFieldStyle(.roundedBorder).accessibilityLabel("源四角")
                TextField("目标四角",text:$destination).textFieldStyle(.roundedBorder).accessibilityLabel("目标四角")
                Button("使用当前画板四角"){artboardCorners()}.buttonStyle(WorkspaceButtonStyle())
            }else{
                pair("平移 X",$x,"Y",$y)
                pair("缩放 X %",$scaleX,"Y %",$scaleY)
                field("旋转 °",$rotation)
                pair("斜切 X °",$shearX,"Y °",$shearY)
                pair("轴心 X",$pivotX,"Y",$pivotY)
                HStack{Button("水平翻转"){scaleX = -scaleX};Button("垂直翻转"){scaleY = -scaleY}}.buttonStyle(WorkspaceButtonStyle())
                Button("轴心设为画板中心"){if let f=model.snapshot?.frame{pivotX=Double(f.x)+Double(f.width)/2;pivotY=Double(f.y)+Double(f.height)/2}}.buttonStyle(WorkspaceButtonStyle())
            }
            Picker("重采样",selection:$interpolation){Text("最近邻（保留整数采样）").tag("nearest");Text("双线性（线性光）").tag("bilinear")}
            }.disabled(preview.applying)
            TonalPreviewControls(preview:preview,parameters:parameters.flatMap{try? commandJSON($0)},filterCommand:"transform.apply").disabled(!available)
            HStack{
                Button("应用变换"){commit()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!available||parameters == nil||preview.applying)
                Button("取消预览"){preview.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!preview.enabled && !preview.busy && !preview.applying)
            }
            if !available{Text("请选择未锁定像素层；选区模式需要非空选区。生成图层和图层组的自由变换仍待支持。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
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
    private func field(_ title:String,_ value:Binding<Double>)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel("变换"+title)}}
    private func pair(_ a:String,_ av:Binding<Double>,_ b:String,_ bv:Binding<Double>)->some View{HStack{field(a,av);field(b,bv)}}
    private func artboardCorners(){guard let f=model.snapshot?.frame else{return};let x=Double(f.x),y=Double(f.y),r=x+Double(f.width),b=y+Double(f.height);source="\(x),\(y); \(r),\(y); \(r),\(b); \(x),\(b)";destination=source}
    private func commit(){guard let layer=model.selected,var parameters else{return};if preview.enabled,let json=try? commandJSON(parameters){preview.apply(json);return};parameters["layer"]=["id":layer.id];model.send(.command("transform.apply",parameters))}
}

private struct CanvasTransformControls:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var input:CanvasTransformInput
    private var available:Bool{model.permissions.edits&&model.selected?.kind?.kind=="pixel"&&model.selected?.visible==true&&model.selected?.locks?.all != true&&model.selected?.locks?.pixels != true&&model.selected?.locks?.position != true&&model.selected?.locks?.transparency != true && (model.canvasTransformScope != "selection" || model.snapshot?.selectionExtent != nil)}
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text("画布自由变换").fontWeight(.medium)
            Picker("变换范围",selection:$model.canvasTransformScope){Text("整个像素图层").tag("whole_layer");Text("选区内像素").tag("selection")}.disabled(input.hasDraft||input.running)
            Picker("手柄模式",selection:$model.canvasTransformMode){ForEach(CanvasTransformMode.allCases,id:\.self){Text($0.title).tag($0.rawValue)}}.disabled(input.running)
            Picker("重采样",selection:$model.canvasTransformInterpolation){Text("最近邻").tag("nearest");Text("双线性").tag("bilinear")}.disabled(input.running)
            Text("拖动角点或边缘手柄，框内拖动平移；上方圆点旋转。Shift 约束比例、方向或 15° 旋转，Option 围绕中心缩放。透视模式用角点控制对应边。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Text("拖动时显示变换轮廓，图像在应用后更新。方向键微移，Shift 加速；回车应用，Esc 取消。选区模式同步变换选中像素与选区。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            HStack{
                Button(input.hasDraft ? "重新开始":"读取边界"){input.start()}.buttonStyle(WorkspaceButtonStyle()).disabled(!available||input.running)
                Button("应用变换"){_ = input.confirm()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!input.canConfirm)
                Button("取消"){input.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!input.hasDraft && !input.running)
            }
            if !input.error.isEmpty{Text(input.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
            if !available && !input.running{Text("请选择可编辑的可见像素图层；选区范围需要非空选区。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
        }.onChange(of:model.canvasTransformMode){_,_ in model.send(.configureCanvasTransform)}
            .onChange(of:model.canvasTransformScope){_,_ in model.send(.configureCanvasTransform)}
            .onChange(of:model.canvasTransformInterpolation){_,_ in model.send(.configureCanvasTransform)}
    }
}
