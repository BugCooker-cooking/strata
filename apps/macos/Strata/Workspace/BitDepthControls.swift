import SwiftUI

struct BitDepthPanel:View{
    @ObservedObject var model:WorkspaceModel
    @State private var depth:UInt8=16
    private var signature:String{"\(model.snapshot?.documentId ?? ""):\(model.snapshot?.bitDepth ?? 0)"}
    var body:some View{
        VStack(alignment:.leading,spacing:8){
            Text(L10n.text("文档位深")).fontWeight(.medium)
            Text(L10n.text("转换整份文档，保留独立图层、蒙版和色彩配置。降低为 8 位会量化颜色与透明度，可撤销恢复。")).font(.system(size:10)).foregroundStyle(Color(nsColor:WorkspaceColors.secondary))
            Picker(L10n.text("目标位深"),selection:$depth){Text(L10n.text("8 位 / 通道")).tag(UInt8(8));Text(L10n.text("16 位 / 通道")).tag(UInt8(16))}
            Button(L10n.text("转换文档位深")){model.send(.command("image.convert_bit_depth",["bit_depth":Int(depth)]))}.buttonStyle(WorkspaceButtonStyle(selected:true)).disabled(depth==model.snapshot?.bitDepth)
        }.disabled(!model.permissions.edits || model.snapshot?.frame == nil)
            .onAppear{readDepth()}.onChange(of:signature){_,_ in readDepth()}
    }
    private func readDepth(){if let value=model.snapshot?.bitDepth{depth=value}}
}
