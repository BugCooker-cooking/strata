import Foundation
import SwiftUI

/// One worker owns a display preview. New input publishes a cancellation
/// generation; it never waits for composition or adds document history.
@MainActor
final class TonalPreviewModel: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var busy = false
    @Published private(set) var applying = false
    @Published private(set) var error = ""
    private let engine: StrataEngine
    private var context: (document: String?, revision: UInt64, layer: String?, control: String)?
    private var pending: (settings: String, sequence: UInt64)?
    private var displayed:(settings:String,sequence:UInt64)?
    private var requestedSequence:UInt64?
    private var filterCommand:String?
    private enum Ending { case cancel(UInt64), apply(String, UInt64) }
    private var ending: Ending?
    private var worker: Task<Void, Never>?
    private var begun = false
    var onBusy: ((Bool) -> Void)?
    var onApplying: ((Bool) -> Void)?
    var onFrame: (() -> Void)?
    var onStarting:(()->Void)?

    init(engine: StrataEngine) { self.engine = engine }
    func configureFilter(_ command:String?){
        guard !applying,filterCommand != command else{return}
        cancel();filterCommand=command;displayed=nil
    }
    func bind(_ snapshot: EditorSnapshot) {
        let changed = context?.document != snapshot.documentId || context?.revision != snapshot.revision
            || context?.layer != snapshot.activeLayer || context?.control != (snapshot.control?.kind ?? "user")
        if changed { cancel() }
        context = (snapshot.documentId, snapshot.revision, snapshot.activeLayer, snapshot.control?.kind ?? "user")
    }
    func setEnabled(_ value: Bool, settings: String?) {
        if !value { cancel(); return }
        guard !applying, context?.control == "user", context?.layer != nil else { return }
        if !enabled{onStarting?()}
        enabled = true; error = ""; update(settings)
    }
    func update(_ settings: String?) {
        guard enabled, !applying else { return }
        do {
            let sequence = try engine.issueTonalPreview()
            requestedSequence=sequence
            guard let settings else { error = "参数格式无效；请检查数值。"; pending = nil; return }
            pending = (settings, sequence); error = ""; drain()
        } catch { self.error = error.localizedDescription }
    }
    func cancel() {
        guard enabled || worker != nil || begun else { return }
        enabled = false; pending = nil;displayed=nil
        do {let sequence=try engine.issueTonalPreview();requestedSequence=sequence;ending = .cancel(sequence);drain()}
        catch { self.error = error.localizedDescription }
    }
    func apply(_ settings: String) {
        guard enabled, !applying else { return }
        do {
            let sequence:UInt64
            if filterCommand != nil,!busy,pending == nil,let displayed,displayed.settings == settings,displayed.sequence == requestedSequence{sequence=displayed.sequence}
            else{sequence=try engine.issueTonalPreview();requestedSequence=sequence}
            enabled = false; applying = true; onApplying?(true); pending = nil
            ending = .apply(settings, sequence); drain()
        } catch { self.error = error.localizedDescription }
    }
    private func drain() {
        guard worker == nil else { return }
        let engine = engine
        busy = true; onBusy?(true)
        worker = Task { [weak self] in
            guard let self else { return }
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
                        case .cancel(let sequence):
                            try await aiEngineWork { try engine.endTonalPreviewChecked(settingsJson: "{}", cancel: true,sequence:sequence) }
                        case .apply(let parameters, let sequence):
                            if !self.begun {
                                guard let context = self.context, let layer = context.layer else { break }
                                try await self.begin(layer:layer,revision:context.revision,sequence:sequence)
                                self.begun=true
                            }
                            if self.filterCommand != nil{try await aiEngineWork{try engine.endFilterPreviewChecked(paramsJson:parameters,cancel:false,sequence:sequence)}}
                            else{try await aiEngineWork { try engine.endTonalPreviewChecked(settingsJson: parameters, cancel: false,sequence:sequence) }}
                        }
                    } catch { self.record(error) }
                    self.begun = false;self.displayed=nil
                    if !self.enabled {if self.ending != nil{continue};break}
                    if let pending = self.pending {
                        do {let sequence=try engine.issueTonalPreview();self.requestedSequence=sequence;self.pending = (pending.settings,sequence)}
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
                        try await self.begin(layer:layer,revision:context.revision,sequence:next.sequence)
                        self.begun = true
                    }
                    if self.filterCommand != nil{try await aiEngineWork{try engine.previewFilter(paramsJson:next.settings,sequence:next.sequence)}}
                    else{try await aiEngineWork { try engine.previewTonal(settingsJson: next.settings, sequence: next.sequence) }}
                    self.displayed=next
                    self.onFrame?()
                } catch { self.record(error) }
            }
        }
    }
    private func begin(layer:String,revision:UInt64,sequence:UInt64)async throws{
        let engine=engine
        if let command=filterCommand{try await aiEngineWork{try engine.beginFilterPreview(layerId:layer,command:command,revision:revision,sequence:sequence)}}
        else{try await aiEngineWork{try engine.beginTonalPreview(layerId:layer,revision:revision,sequence:sequence)}}
    }
    private func record(_ error: Error) {
        if let strataError = error as? StrataError, case .Cancelled = strataError { return }
        self.error = (error as? StrataError)?.userMessage ?? error.localizedDescription
    }
}

struct TonalPreviewControls: View {
    @ObservedObject var preview: TonalPreviewModel
    let parameters: String?
    var filterCommand:String?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Toggle("画布预览", isOn: Binding(get: { preview.enabled }, set: { preview.setEnabled($0, settings: parameters) }))
                    .disabled(preview.applying)
                if preview.busy { ProgressView().controlSize(.small) }
            }
            if preview.enabled { Text("预览尚未写入文档；点击应用保存，关闭预览恢复原效果。").font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.secondary)) }
            if !preview.error.isEmpty { Text(preview.error).font(.system(size: 10)).foregroundStyle(Color(nsColor: WorkspaceColors.danger)).textSelection(.enabled) }
        }.onAppear{preview.configureFilter(filterCommand)}
            .onChange(of:filterCommand){_,value in preview.configureFilter(value)}
            .onChange(of: parameters) { _, value in preview.update(value) }
            .onDisappear { preview.cancel() }
    }
}
