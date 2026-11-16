import AppKit

/// Pure scheduling state, independent of timers and wall-clock adjustments.
struct RecoverySchedule {
    private(set) var revision: UInt64?
    private(set) var due: TimeInterval?
    private(set) var running = false
    private var dirty = false
    mutating func observe(revision: UInt64, dirty: Bool, now: TimeInterval) {
        let becameDirty = dirty && !self.dirty; self.dirty = dirty
        if !dirty { self.revision = revision; due = nil; return }
        if self.revision != revision || becameDirty { self.revision = revision; due = now + 30 }
    }
    mutating func begin(now: TimeInterval, busy: Bool) -> UInt64? {
        guard !running, !busy, let due, now >= due, let revision else { return nil }
        running = true; return revision
    }
    mutating func finish(revision: UInt64, success: Bool, now: TimeInterval) {
        running = false
        if self.revision == revision, due != nil { due = success ? nil : now + 30 }
    }
}

struct RecoveryEntry: Decodable, Sendable {
    struct Info: Decodable, Sendable { let label: String; let savedUnixSeconds: UInt64 }
    let id: String
    let path: String
    let info: Info?
    let error: String?
    static func decode(_ json: String) throws -> [RecoveryEntry] {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode([Self].self, from: Data(json.utf8))
    }
}

@MainActor
final class RecoveryMonitor {
    private let engine: StrataEngine
    private weak var controller: MainWindowController?
    private var schedule = RecoverySchedule()
    private var timer: Timer?
    private var wasDirty = false
    var recoveredEntry: String?
    init(engine: StrataEngine, controller: MainWindowController) {
        self.engine = engine; self.controller = controller
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        self.timer = timer; RunLoop.main.add(timer, forMode: .common)
    }
    func observe(_ snapshot: EditorSnapshot) {
        let becameClean = wasDirty && !snapshot.dirty
        wasDirty = snapshot.dirty
        schedule.observe(revision: snapshot.revision, dirty: snapshot.dirty, now: ProcessInfo.processInfo.systemUptime)
        if !snapshot.dirty {
            controller?.workspace.recoveryStatus = ""
            if becameClean {
                let engine = engine, old = recoveredEntry; recoveredEntry = nil
                DispatchQueue.global(qos: .utility).async { try? engine.clearRecoveryIfClean(); if let old { try? engine.discardRecovery(id: old) } }
            }
        } else if schedule.due != nil {
            controller?.workspace.recoveryStatus = L10n.text("等待恢复检查点")
        }
    }
    private func tick() {
        guard let controller, let revision = schedule.begin(now: ProcessInfo.processInfo.systemUptime, busy: controller.busy) else { return }
        controller.workspace.recoveryStatus = L10n.text("正在写入恢复副本…")
        let engine = engine, label = controller.fileName
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let result = Result { try engine.checkpointRecovery(label: label) }
            DispatchQueue.main.async {
                guard let self else { return }
                self.schedule.finish(revision: revision, success: (try? result.get()) != nil, now: ProcessInfo.processInfo.systemUptime)
                switch result {
                case .success(let json):
                    if json != "null", self.schedule.revision == revision, self.schedule.due == nil {
                        let time = DateFormatter.localizedString(from: Date(), dateStyle: .none, timeStyle: .short)
                        self.controller?.workspace.recoveryStatus = L10n.format("恢复副本 {0}", String(describing: time))
                        self.controller?.workspace.recoveryError = ""
                        // Keep the original crash copy until a replacement exists.
                        if let id = self.recoveredEntry { try? engine.discardRecovery(id: id); self.recoveredEntry = nil }
                    }
                case .failure(let error):
                    self.controller?.workspace.recoveryStatus = L10n.text("恢复副本写入失败")
                    self.controller?.workspace.recoveryError = error.localizedDescription
                }
            }
        }
    }
    func stop(discard: Bool, completion: @escaping @MainActor @Sendable () -> Void = {}) {
        timer?.invalidate(); timer = nil
        guard discard else { completion(); return }
        let engine = engine, old = recoveredEntry
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let result = Result { try engine.discardRecovery(id: nil); if let old { try engine.discardRecovery(id: old) } }
            DispatchQueue.main.async {
                if case .failure = result { self?.controller?.workspace.recoveryStatus = L10n.text("恢复副本清理失败") }
                completion()
            }
        }
    }
}
