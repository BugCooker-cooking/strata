import Foundation
import SwiftUI

/// One worker owns a display preview. Registering newer input only increments
/// an atomic host sequence; it never waits for the renderer or adds history.
@MainActor
final class TonalPreviewModel: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var busy = false
    @Published private(set) var applying = false
    @Published private(set) var error = ""
    private let engine: StrataEngine
    private var context: (document: String?, revision: UInt64, layer: String?, control: String)?
    private var pending: (settings: String, sequence: UInt64)?
    private enum Ending { case cancel, apply(String, UInt64) }
    private var ending: Ending?
    private var worker: Task<Void, Never>?
    private var begun = false
    var onBusy: ((Bool) -> Void)?
    var onApplying: ((Bool) -> Void)?
    var onFrame: (() -> Void)?

    init(engine: StrataEngine) { self.engine = engine }
    func bind(_ snapshot: EditorSnapshot) {
        let changed = context?.document != snapshot.documentId || context?.revision != snapshot.revision
            || context?.layer != snapshot.activeLayer || context?.control != (snapshot.control?.kind ?? "user")
        if changed { cancel() }
        context = (snapshot.documentId, snapshot.revision, snapshot.activeLayer, snapshot.control?.kind ?? "user")
    }
    func setEnabled(_ value: Bool, settings: String?) {
        if !value { cancel(); return }
        guard !applying, context?.control == "user", context?.layer != nil else { return }
        enabled = true; error = ""; update(settings)
    }
    func update(_ settings: String?) {
        guard enabled, !applying else { return }
        do {
            let sequence = try engine.issueTonalPreview()
            guard let settings else { error = "参数格式无效；请检查数值。"; pending = nil; return }
            pending = (settings, sequence); error = ""; drain()
        } catch { self.error = error.localizedDescription }
    }
    func cancel() {
        guard enabled || worker != nil || begun else { return }
        enabled = false; pending = nil
        do { _ = try engine.issueTonalPreview() } catch { self.error = error.localizedDescription }
        if applying { try? engine.cancelPendingWork(); return }
        ending = .cancel; drain()
    }
    func apply(_ settings: String) {
        guard enabled, !applying else { return }
        do {
            let sequence = try engine.issueTonalPreview()
            enabled = false; applying = true; onApplying?(true); pending = nil
            ending = .apply(settings, sequence); drain()
        } catch { self.error = error.localizedDescription }
    }
    private func drain() {
        guard worker == nil else { return }
        let engine = engine
        worker = Task { [weak self] in
            guard let self else { return }
            self.busy = true; self.onBusy?(true)
            defer {
                self.worker = nil; self.busy = false; self.onBusy?(false)
                if self.applying { self.applying = false; self.onApplying?(false) }
                self.onFrame?()
            }
            while true {
                if let ending = self.ending {
                    self.ending = nil
                    do {
                        switch ending {
                        case .cancel:
                            if self.begun { try await aiEngineWork { try engine.endTonalPreview(settingsJson: "{}", cancel: true) } }
                        case .apply(let parameters, let sequence):
                            if !self.begun {
                                guard let context = self.context, let layer = context.layer else { break }
                                try await aiEngineWork { try engine.beginTonalPreview(layerId: layer, revision: context.revision, sequence: sequence) }
                            }
                            try await aiEngineWork { try engine.endTonalPreview(settingsJson: parameters, cancel: false) }
                        }
                    } catch { self.record(error) }
                    self.begun = false
                    if !self.enabled { break }
                    if let pending = self.pending {
                        do { self.pending = (pending.settings, try engine.issueTonalPreview()) }
                        catch { self.record(error); break }
                    }
                }
                guard self.enabled, self.pending != nil else { break }
                // Coalesce text edits, curve moves and channel switches. Only
                // the latest sequence is prepared after this bounded delay.
                try? await Task.sleep(for: .milliseconds(120))
                guard self.ending == nil, self.enabled, let next = self.pending,
                      let context = self.context, let layer = context.layer else { continue }
                self.pending = nil
                do {
                    if !self.begun {
                        try await aiEngineWork { try engine.beginTonalPreview(layerId: layer, revision: context.revision, sequence: next.sequence) }
                        self.begun = true
                    }
                    try await aiEngineWork { try engine.previewTonal(settingsJson: next.settings, sequence: next.sequence) }
                    self.onFrame?()
                } catch { self.record(error) }
            }
        }
    }
    private func record(_ error: Error) {
        if let strataError = error as? StrataError, case .Cancelled = strataError { return }
        self.error = (error as? StrataError)?.userMessage ?? error.localizedDescription
    }
}

struct TonalPreviewControls: View {
    @ObservedObject var preview: TonalPreviewModel
    let parameters: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Toggle("画布预览", isOn: Binding(get: { preview.enabled }, set: { preview.setEnabled($0, settings: parameters) }))
                    .disabled(preview.applying)
                if preview.busy { ProgressView().controlSize(.small) }
            }
            if preview.enabled { Text("预览尚未写入文档；点击应用保存，关闭预览恢复原效果。").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)) }
            if !preview.error.isEmpty { Text(preview.error).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.danger)).textSelection(.enabled) }
        }.onChange(of: parameters) { _, value in preview.update(value) }
            .onDisappear { preview.cancel() }
    }
}
