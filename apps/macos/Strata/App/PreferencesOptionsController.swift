import AppKit

@MainActor
final class PreferencesOptionsController: NSObject {
    let view = NSStackView()
    let mode = NSPopUpButton()
    let maxSteps = NSTextField()
    let requestTimeout = NSTextField()
    let turnTimeout = NSTextField()
    let cpu = NSTextField()
    let gpu = NSTextField()
    let disk = NSTextField()
    let usage = NSTextField(wrappingLabelWithString: "")
    let status = NSTextField(wrappingLabelWithString: "")
    let applyButton = NSButton(title: "应用偏好", target: nil, action: nil)
    let trimButton = NSButton(title: "释放空闲缓存", target: nil, action: nil)
    let refreshButton = NSButton(title: "刷新占用", target: nil, action: nil)
    private let engine: StrataEngine
    private let preferences: ApplicationPreferences
    private let originalVersion: UInt64
    private(set) var running = false
    var onSaved: (() -> Void)?

    init(engine: StrataEngine, preferences: ApplicationPreferences = .shared) throws {
        self.engine = engine
        self.preferences = preferences
        let current = try preferences.read(from: engine)
        originalVersion = current.version
        super.init()
        mode.addItems(withTitles: ["先问我", "直接执行普通编辑"])
        mode.selectItem(at: current.ai.mode == "ask_first" ? 0 : 1)
        mode.setAccessibilityLabel("AI 默认执行方式")
        maxSteps.stringValue = String(current.ai.maxSteps)
        requestTimeout.stringValue = String(current.ai.requestTimeoutSeconds)
        turnTimeout.stringValue = String(current.ai.turnTimeoutSeconds)
        cpu.stringValue = String(current.memory.cpuMib)
        gpu.stringValue = String(current.memory.gpuMib)
        disk.stringValue = String(current.memory.diskMib)

        view.orientation = .vertical
        view.alignment = .leading
        view.spacing = 10
        view.edgeInsets = NSEdgeInsets(top: 10, left: 10, bottom: 10, right: 10)
        view.frame = NSRect(x: 0, y: 0, width: 510, height: 510)
        view.widthAnchor.constraint(equalToConstant: 510).isActive = true
        addText("AI 默认设置用于下一回合。聊天面板可为单次回合选择执行方式。")
        row("默认执行方式", control: mode)
        row("回合步数 1–200", control: maxSteps)
        row("单请求超时 5–120 秒", control: requestTimeout)
        row("回合超时 30–1800 秒", control: turnTimeout)
        addText("以下预算用于全部文档。MiB 表示 1024×1024 字节。")
        row("CPU 256–16384 MiB", control: cpu)
        row("GPU 64–4096 MiB", control: gpu)
        row("磁盘后备 1024–262144 MiB", control: disk)
        usage.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        usage.setAccessibilityLabel("当前托管资源占用")
        view.addArrangedSubview(usage)
        let actions = NSStackView(views: [applyButton, trimButton, refreshButton])
        actions.spacing = 10
        view.addArrangedSubview(actions)
        status.textColor = .secondaryLabelColor
        status.font = .systemFont(ofSize: 11)
        view.addArrangedSubview(status)
        if !preferences.restoreError.isEmpty { status.stringValue = preferences.restoreError }
        applyButton.target = self
        applyButton.action = #selector(apply)
        trimButton.target = self
        trimButton.action = #selector(trim)
        refreshButton.target = self
        refreshButton.action = #selector(refreshUsage)
        refreshUsage()
    }

    private func addText(_ text: String) {
        let label = NSTextField(wrappingLabelWithString: text)
        label.widthAnchor.constraint(equalToConstant: 480).isActive = true
        label.font = .systemFont(ofSize: 11)
        label.textColor = .secondaryLabelColor
        view.addArrangedSubview(label)
    }

    private func row(_ label: String, control: NSControl) {
        let title = NSTextField(labelWithString: label)
        title.widthAnchor.constraint(equalToConstant: 225).isActive = true
        control.widthAnchor.constraint(equalToConstant: 235).isActive = true
        control.setAccessibilityLabel(label)
        let row = NSStackView(views: [title, control])
        row.spacing = 10
        view.addArrangedSubview(row)
    }

    func parameters() throws -> [String: Any] {
        let inputs = [maxSteps, requestTimeout, turnTimeout, cpu, gpu, disk]
        let values = inputs.compactMap { UInt64($0.stringValue) }
        guard values.count == inputs.count else {
            throw QuickExportError("步数、超时与预算须为正整数。")
        }
        return [
            "expected_version": originalVersion,
            "ai": ["mode": mode.indexOfSelectedItem == 0 ? "ask_first" : "direct", "max_steps": values[0],
                   "request_timeout_seconds": values[1], "turn_timeout_seconds": values[2]],
            "memory": ["cpu_mib": values[3], "gpu_mib": values[4], "disk_mib": values[5]]
        ]
    }

    @objc func refreshUsage() {
        guard !running else { return }
        do {
            let json = try AIJSON.decode(engine.dispatch(command: "system.memory.get", paramsJson: "{}"))
            updateUsage(json)
        } catch { show(error) }
    }

    private func updateUsage(_ json: AIJSON) {
        let used = json["usage"]
        let limits = json["limits"]
        func mib(_ number: AIJSON) -> String { String(format: "%.1f", (number.number ?? 0) / 1048576) }
        usage.stringValue = "托管占用 / 额度 MiB\nCPU \(mib(used["cpu_bytes"])) / \(mib(limits["cpu_bytes"]))    GPU \(mib(used["gpu_bytes"])) / \(mib(limits["gpu_bytes"]))\n磁盘后备 \(mib(used["disk_bytes"])) / \(mib(limits["disk_bytes"]))"
    }

    @objc func apply() {
        guard !running else { return }
        do { run("system.preferences.set", parameters: try commandJSON(parameters()), saving: true) }
        catch { show(error) }
    }

    @objc func trim() {
        guard !running else { return }
        run("system.memory.trim", parameters: "{}", saving: false)
    }

    private func run(_ command: String, parameters: String, saving: Bool) {
        running = true
        applyButton.isEnabled = false
        trimButton.isEnabled = false
        refreshButton.isEnabled = false
        let engine = engine
        Task { [weak self] in
            guard let self else { return }
            var committed = false
            defer {
                self.running = false
                self.applyButton.isEnabled = true
                self.trimButton.isEnabled = true
                self.refreshButton.isEnabled = true
            }
            do {
                let result = try await aiEngineWork { try engine.dispatch(command: command, paramsJson: parameters) }
                committed = true
                if saving {
                    try self.preferences.save(from: engine)
                    self.onSaved?()
                } else {
                    self.updateUsage(try AIJSON.decode(result))
                    self.status.stringValue = "已释放空闲资源。使用中的分配继续保留。"
                }
            } catch {
                if saving, !committed,
                   let current = try? self.preferences.read(from: engine),
                   current.version != self.originalVersion {
                    self.status.stringValue = "偏好设置已在其他标签页更新，请关闭后重新打开本面板。"
                } else { self.show(error) }
            }
        }
    }

    private func show(_ error: Error) {
        guard let engineError = error as? StrataError else {
            status.stringValue = error.localizedDescription
            return
        }
        if case .Command(let message) = engineError {
            if message.contains("AI steps must") {
                status.stringValue = "AI 步数须为 1–200，单请求超时为 5–120 秒，回合超时为 30–1800 秒。"
            } else if message.contains("memory budgets in MiB require") {
                status.stringValue = "内存额度须在面板标注的范围内：CPU 256–16384、GPU 64–4096、磁盘后备 1024–262144 MiB。"
            } else if message.contains("budget exhausted") {
                status.stringValue = "现有资源需要更高额度。可先释放空闲缓存，或提高额度后重试。"
            } else {
                status.stringValue = "偏好操作未完成。详细原因：\(message)"
            }
        } else {
            status.stringValue = engineError.userMessage
        }
    }
}
