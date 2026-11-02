import AppKit
import UniformTypeIdentifiers

@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate {
    let engine: StrataEngine
    let canvas: CanvasView
    private let status = NSTextField(labelWithString: "M0 技术验证 · 只读图像预览")
    private var requestID: UInt64 = 0
    private var lastGeneration: UInt64 = 0

    init(engine: StrataEngine) {
        self.engine = engine
        canvas = CanvasView(engine: engine)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init(window: window)
        window.title = "Strata"
        window.subtitle = "M0 · 图片与色彩管线验证"
        window.minSize = NSSize(width: 960, height: 600)
        window.toolbarStyle = .unified
        window.isReleasedWhenClosed = false
        window.delegate = self
        let content = NSView()
        window.contentView = content
        let openButton = NSButton(title: "打开图片…", target: self, action: #selector(openImage))
        let fitButton = NSButton(title: "适合屏幕", target: self, action: #selector(fit))
        status.textColor = .secondaryLabelColor
        let header = NSStackView(views: [openButton, fitButton, status])
        header.spacing = 12
        header.orientation = .horizontal
        content.addSubview(header)
        content.addSubview(canvas)
        header.translatesAutoresizingMaskIntoConstraints = false
        canvas.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            header.topAnchor.constraint(equalTo: content.topAnchor, constant: 10),
            header.trailingAnchor.constraint(lessThanOrEqualTo: content.trailingAnchor, constant: -12),
            canvas.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            canvas.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            canvas.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 10),
            canvas.bottomAnchor.constraint(equalTo: content.bottomAnchor)
        ])
        canvas.onError = { [weak self] error in self?.showError(error) }
        window.center()
    }

    required init?(coder: NSCoder) { nil }

    func handle(_ event: EngineEventDto) {
        if case let .documentOpened(info, generation) = event {
            guard generation >= lastGeneration else { return }
            lastGeneration = generation
            window?.subtitle = "\(info.width) × \(info.height) · \(info.sourceProfile) · \(info.bitDepth) 位"
            status.stringValue = info.profileFallback ? "ICC 损坏：按 sRGB 显示 · 原始数据保留" : "M0 技术验证 · 拖动/滚动平移，捏合缩放"
            canvas.markDirty()
        }
    }

    func open(_ url: URL, completion: ((Result<ImageInfo, Error>) -> Void)? = nil) {
        canvas.cancelBenchmarkForDocumentChange()
        requestID += 1
        let current = requestID
        status.stringValue = "正在解码与准备显示…"
        let engine = engine
        Task { [weak self] in
            do {
                let info = try await Task.detached(priority: .userInitiated) { try engine.openImage(path: url.path) }.value
                guard let self, self.requestID == current else { return }
                self.window?.title = "\(url.lastPathComponent) — Strata"
                completion?(.success(info))
            } catch {
                guard let self, self.requestID == current else { return }
                if case StrataError.Cancelled = error { completion?(.failure(error)); return }
                self.status.stringValue = "打开失败 · 保留原来的图像"
                if let completion { completion(.failure(error)) } else { self.showError(error) }
            }
        }
    }

    @objc func openImage() {
        guard let window else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = ["png", "jpg", "jpeg", "webp", "tiff", "tif", "gif", "bmp"].compactMap { UTType(filenameExtension: $0) }
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.open(url)
        }
    }

    @objc func fit() { canvas.fit() }
    @objc func zoomIn() { canvas.zoomBy(1.25) }
    @objc func zoomOut() { canvas.zoomBy(0.8) }
    func windowWillClose(_ notification: Notification) { canvas.shutdown() }
    func windowDidDeminiaturize(_ notification: Notification) { canvas.markDirty() }

    private func showError(_ error: Error) {
        guard let window, window.attachedSheet == nil else { return }
        let alert = NSAlert()
        alert.messageText = "无法打开或显示图片"
        alert.informativeText = (error as? StrataError)?.userMessage
            ?? "操作未能完成，请稍后重试。原来的图像会保留。"
        alert.beginSheetModal(for: window)
    }
}
