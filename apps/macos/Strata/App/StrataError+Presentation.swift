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
        case .Internal, .NoCanvas:
            return "画布暂时不可用，请重新启动应用后再试。"
        }
    }
}
