import Foundation

extension StrataError {
    var userMessage: String {
        switch self {
        case .NotFound:
            return "找不到这张图片。它可能已被移动或删除，请重新选择文件。"
        case .UnsupportedFormat:
            return "无法识别图片内容，或图片格式暂不支持。请尝试 PNG、JPEG、WebP、TIFF、GIF 或 BMP 文件。"
        case .Decode:
            return "无法读取图片。文件可能已损坏、超出当前大小限制，或使用了暂不支持的编码。"
        case .Color:
            return "无法处理图片的色彩配置。请尝试转换为 RGB 格式后重新打开。"
        case .Render:
            return "无法准备图片画面。图片可能超出显卡限制，或图形资源暂时不可用。请尝试较小的图片，或重新启动应用。"
        case .Cancelled:
            return "操作已取消。"
        case .UnsavedChanges:
            return "当前文档有未保存修改，请先保存或确认放弃修改。"
        case .Command(let message):
            if message.contains("resource limit") || message.contains("pixel budget") {
                return "文档超过当前阶段的容量限制。请减少图层或图片尺寸后重试；原文档和撤销记录已保留。"
            }
            if message.contains("nothing to undo") { return "目前没有可撤销的操作。" }
            if message.contains("nothing to redo") { return "目前没有可重做的操作。" }
            if message.contains("durability could not be confirmed") {
                return "文件已经写入，但系统未能确认它已安全保存到磁盘。文档仍标记为未保存，请另存一份并检查结果。"
            }
            if message.contains("permission") || message.contains("explicit overwrite") {
                return "文件授权已失效，或目标文件发生变化。请重新选择文件，再尝试保存。"
            }
            if message.contains("project") || message.contains("checksum") {
                return "工程文件无法读取或保存。它可能已损坏、版本不兼容，或存储空间不足。原文档已保留。"
            }
            return "无法完成这项操作，请检查图层参数与文件位置。详细原因：\(message)"
        case .Internal, .NoCanvas:
            return "画布暂时不可用，请重新启动应用后再试。"
        }
    }
}
