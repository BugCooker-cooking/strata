import SwiftUI

struct FilterPanel:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var preview:TonalPreviewModel
    @State private var command="filter.gaussian_blur"
    @State private var sigma=2.0
    @State private var amount=100.0
    @State private var threshold=0.0
    @State private var noiseAmount=2.0
    @State private var seed="0"
    @State private var distribution="uniform"
    @State private var monochrome=false
    private var parameters:[String:Any]?{
        if command == "filter.add_noise"{
            guard let seed=UInt32(seed),noiseAmount.isFinite,(0...100).contains(noiseAmount)else{return nil}
            return ["amount":noiseAmount/100,"seed":seed,"distribution":distribution,"monochrome":monochrome]
        }
        guard sigma.isFinite,(0.1...64).contains(sigma)else{return nil}
        if command == "filter.unsharp_mask"{
            guard amount.isFinite,(0...500).contains(amount),threshold.isFinite,(0...100).contains(threshold)else{return nil}
            return ["sigma":sigma,"amount":amount/100,"threshold":threshold/100]
        }
        return ["sigma":sigma]
    }
    private var available:Bool{model.selected?.kind?.kind == "pixel"&&model.permissions.edits&&model.selected?.locks?.all != true&&model.selected?.locks?.pixels != true}
    var body:some View{
        ScrollView{VStack(alignment:.leading,spacing:10){
            Group{
            Picker("滤镜",selection:$command){Text("高斯模糊").tag("filter.gaussian_blur");Text("USM 锐化").tag("filter.unsharp_mask");Text("添加杂色").tag("filter.add_noise")}.labelsHidden()
            if command == "filter.add_noise"{
                number("数量",value:$noiseAmount,unit:"%")
                Picker("分布",selection:$distribution){Text("均匀").tag("uniform");Text("高斯").tag("gaussian")}
                Toggle("单色",isOn:$monochrome)
                TextField("随机种子",text:$seed).textFieldStyle(.roundedBorder).accessibilityLabel("杂色随机种子")
                Text("相同种子保留相同颗粒分布。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }else{
                number("标准差",value:$sigma,unit:"px")
                Slider(value:$sigma,in:0.1...64).accessibilityLabel("模糊标准差")
                if command == "filter.unsharp_mask"{
                    number("数量",value:$amount,unit:"%")
                    number("阈值",value:$threshold,unit:"%")
                    Text("阈值按线性 RGB 差值计算，保留原透明度。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
                }
            }
            }.disabled(!available)
            TonalPreviewControls(preview:preview,parameters:parameters.flatMap{try? commandJSON($0)},filterCommand:command).disabled(!available)
            HStack{
                Button("应用"){commit()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!available||parameters == nil||preview.applying)
                Button("取消预览"){preview.cancel()}.buttonStyle(WorkspaceButtonStyle()).disabled(!preview.enabled && !preview.busy && !preview.applying)
            }
            if !available && !preview.applying{Text("请选择可编辑的像素图层。").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
        }.padding(12)}
    }
    private func number(_ title:String,value:Binding<Double>,unit:String)->some View{HStack{Text(title);TextField(title,value:value,format:.number.precision(.fractionLength(2))).textFieldStyle(.roundedBorder).accessibilityLabel("滤镜"+title);Text(unit)}}
    private func commit(){
        guard let layer=model.selected,var parameters else{return}
        if preview.enabled,let json=try? commandJSON(parameters){preview.apply(json);return}
        parameters["layer"]=["id":layer.id];model.send(.command(command,parameters))
    }
}
