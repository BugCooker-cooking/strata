import Foundation

extension StrataError {
    var userMessage: String {
        switch self {
        case .NotFound:
            return L10n.text("找不到这张图片。它可能已被移动或删除，请重新选择文件。")
        case .UnsupportedFormat(let message):
            switch message {
            case "unsupported PSD feature: layer block TySh", "unsupported PSD feature: layer block tySh":
                return L10n.text("此 PSD 含有可编辑文字层。请在副本中将文字栅格化后再打开。")
            case "unsupported PSD feature: layer block SoCo", "unsupported PSD feature: layer block vmsk", "unsupported PSD feature: layer block vsms":
                return L10n.text("此 PSD 含有填充图层或矢量蒙版。请在副本中将相应图层栅格化后再打开。")
            case "unsupported PSD feature: layer block SoLd", "unsupported PSD feature: layer block SoLE", "unsupported PSD feature: layer block PlLd":
                return L10n.text("此 PSD 含有智能对象。请在副本中将智能对象栅格化后再打开。")
            default:
                break
            }
            if let detail = message.split(separator: ":", maxSplits: 1).last, message.hasPrefix("unsupported PSD feature:") {
                return L10n.format("此 PSD 含有暂不支持的内容（{0}）。请在副本中转换该内容后再打开。", String(detail).trimmingCharacters(in: .whitespaces))
            }
            if message.contains("unsupported pixel format") {
                return L10n.text("此图片使用不支持的颜色模式或样本格式。请将副本转换为每通道 8 位或 16 位的 RGB，再重新打开。")
            }
            return L10n.text("无法识别图片内容，或图片格式暂不支持。请尝试 PNG、JPEG、WebP、TIFF、GIF 或 BMP 文件。")
        case .Decode:
            return L10n.text("无法读取图片。文件可能已损坏、超出当前大小限制，或使用了暂不支持的编码。")
        case .Color:
            return L10n.text("无法处理图片的色彩配置。请尝试转换为 RGB 格式后重新打开。")
        case .Render:
            return L10n.text("无法准备图片画面。图片可能超出显卡限制，或图形资源暂时不可用。请尝试较小的图片，或重新启动应用。")
        case .Cancelled:
            return L10n.text("操作已取消。")
        case .UnsavedChanges:
            return L10n.text("当前文档有未保存修改，请先保存或确认放弃修改。")
        case .Command(let message):
            if let marker = message.range(of: "PSD export cannot represent this layer: ") {
                return L10n.format("图层“{0}”含有当前 PSD 导出无法保留的内容。请在副本中将它栅格化后重试。", String(message[marker.upperBound...]))
            }
            if message.contains("PSD export needs a 16-bit document") {
                return L10n.text("蒙版包含高于 8 位精度的样本。请将文档副本转为 16 位，再导出分层 PSD。")
            }
            if message.contains("glyphs are not present in the bundled fonts") {
                return L10n.text("内置字体缺少此文本中的部分字符。请使用受支持的字符，或在文字面板中选用其他字体。")
            }
            if message.contains("resource limit") || message.contains("pixel budget") {
                return L10n.text("文档超过当前阶段的容量限制。请减少图层或图片尺寸后重试；原文档和撤销记录已保留。")
            }
            if message.contains("nothing to undo") { return L10n.text("目前没有可撤销的操作。") }
            if message.contains("nothing to redo") { return L10n.text("目前没有可重做的操作。") }
            if message.contains("durability could not be confirmed") {
                return L10n.text("文件已经写入，但系统未能确认它已安全保存到磁盘。文档仍标记为未保存，请另存一份并检查结果。")
            }
            if message.contains("permission") || message.contains("explicit overwrite") {
                return L10n.text("文件授权已失效，或目标文件发生变化。请重新选择文件，再尝试保存。")
            }
            if message.contains("project") || message.contains("checksum") {
                return L10n.text("工程文件无法读取或保存。它可能已损坏、版本不兼容，或存储空间不足。原文档已保留。")
            }
            return L10n.format("无法完成这项操作，请检查图层参数与文件位置。详细原因：{0}", String(describing: message))
        case .Internal, .NoCanvas:
            return L10n.text("画布暂时不可用，请重新启动应用后再试。")
        }
    }
}
