import SwiftUI

struct ColorPanelView:View{
    @ObservedObject var model:WorkspaceModel
    @ObservedObject var panel:ColorPanelModel
    @State private var draft=ColorPanelDraft()
    @State private var baseline=ColorPanelDraft()
    private var depth:UInt8{model.snapshot?.bitDepth ?? 8}
    private var validInput:[String:Any]?{draft.colorInput(depth:depth)}
    var body:some View{
        ScrollView{VStack(alignment:.leading,spacing:10){
            Text(L10n.text("颜色")).fontWeight(.medium)
            HStack{
                RoundedRectangle(cornerRadius:4).fill(Color(nsColor:model.foregroundColor)).frame(width:30,height:24).overlay(RoundedRectangle(cornerRadius:4).stroke(Color(nsColor:WorkspaceColors.border)))
                RoundedRectangle(cornerRadius:4).fill(Color(nsColor:model.backgroundColor)).frame(width:30,height:24).overlay(RoundedRectangle(cornerRadius:4).stroke(Color(nsColor:WorkspaceColors.border)))
                Text(model.snapshot?.profileName ?? "sRGB").font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary)).lineLimit(2)
            }
            Picker(L10n.text("颜色目标"),selection:$draft.target){Text(L10n.text("前景色")).tag("foreground");Text(L10n.text("背景色")).tag("background")}.pickerStyle(.segmented).labelsHidden().accessibilityLabel(L10n.text("颜色目标")).disabled(!model.permissions.edits)
            Picker(L10n.text("颜色数值模式"),selection:$draft.model){Text("RGB").tag("rgb");Text("HSB").tag("hsb");Text("HEX").tag("hex")}.pickerStyle(.segmented).labelsHidden().accessibilityLabel(L10n.text("颜色数值模式"))
            if panel.state == nil{Text(panel.error.isEmpty ? L10n.text("正在读取颜色…"):panel.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            else if draft.model=="hex"{
                TextField(L10n.text("HEX 颜色"),text:$draft.hex).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("HEX 颜色")).onSubmit{apply()}
                Text(L10n.text("#RRGGBB 或 #RRGGBBAA；HEX 使用 8 位编码。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            }else{
                ForEach(0..<3,id:\.self){index in component(index)}
                HStack{Text(L10n.text("透明度"));Slider(value:Binding(get:{draft.alpha.isFinite ? min(100,max(0,draft.alpha)):0},set:{draft.alpha=$0}),in:0...100,step:0.1);TextField(L10n.text("颜色透明度"),value:$draft.alpha,format:.number.precision(.fractionLength(2))).frame(width:64).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.text("颜色透明度")).onSubmit{apply()};Text("%")}
            }
            HStack{
                Button(L10n.text("应用颜色数值")){apply()}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(!model.permissions.edits || panel.state == nil || validInput == nil || draft==baseline)
                Button(L10n.text("恢复当前颜色")){read()}.buttonStyle(WorkspaceButtonStyle()).disabled(panel.state == nil)
            }
            if draft != baseline{Text(validInput == nil ? L10n.text("请输入范围内的有效颜色数值。"):L10n.text("颜色数值尚未应用。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))}
            HStack{
                Button(L10n.text("交换前景背景")){model.swapPaintColors()}.buttonStyle(WorkspaceButtonStyle())
                Button(L10n.text("重置黑白")){model.send(.command("paint.colors.reset",[:]))}.buttonStyle(WorkspaceButtonStyle())
            }.disabled(!model.permissions.edits)
            Text(L10n.text("数值使用当前工作 RGB；设置颜色不修改图像，也不进入文档撤销历史。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            if !panel.error.isEmpty{Text(panel.error).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.danger))}
        }.padding(12)}
            .onAppear{read()}.onChange(of:panel.state?.version){_,_ in read()}
            .onChange(of:depth){_,_ in read()}.onChange(of:draft.target){_,_ in read()}.onChange(of:draft.model){_,_ in read()}
    }
    private func component(_ index:Int)->some View{
        let title=(draft.model=="hsb" ? ["H","S","B"]:["R","G","B"])[index]
        let upper=draft.model=="hsb" ? (index==0 ? 360.0:100.0):(depth==16 ? 65535.0:255.0)
        let slider=Binding(get:{draft.values[index].isFinite ? min(upper,max(0,draft.values[index])):0},set:{draft.values[index]=$0})
        return HStack{
            Text(title).frame(width:12)
            Slider(value:slider,in:0...upper,step:draft.model=="rgb" ? 1:0.1)
            TextField(L10n.format("颜色{0}", title),value:Binding(get:{draft.values[index]},set:{value in draft.values[index]=draft.model=="rgb" && value.isFinite ? value.rounded():value}),format:.number.precision(.fractionLength(draft.model=="rgb" ? 0:2))).frame(width:64).textFieldStyle(.roundedBorder).accessibilityLabel(L10n.format("颜色{0}", title)).onSubmit{apply()}
        }.disabled(!model.permissions.edits)
    }
    private func read(){guard let state=panel.state else{return};let next=ColorPanelDraft(state:state,target:draft.target,model:draft.model,depth:depth);draft=next;baseline=next}
    private func apply(){guard model.permissions.edits,let state=panel.state,let input=validInput,draft != baseline else{return};model.send(.command("paint.colors.set_components",["target":draft.target,"color":input,"expected_version":state.version]))}
}
