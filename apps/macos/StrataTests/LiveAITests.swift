import Foundation
import AppKit
import XCTest
@testable import Strata

/// Explicit opt-in integration gate. Uses only the repository's generated
/// sample photo and the already configured service; never prints credentials.
final class LiveAITests: XCTestCase {
    @MainActor
    func testConfiguredServiceWithBothProtocols() async throws {
        guard ProcessInfo.processInfo.environment["STRATA_RUN_LIVE_AI_ACCEPTANCE"] == "1" else { throw XCTSkip("Live model acceptance requires explicit local opt-in.") }
        let settings = AISettingsStore.shared
        let selected = try XCTUnwrap(settings.current, "Configure the service in Strata first.")
        guard URL(string: selected.endpoint)?.host == "121.196.245.65" else { throw XCTSkip("This acceptance run is authorized for the user-specified test endpoint only.") }
        executionTimeAllowance = 1200
        print("STRATA_LIVE reading configured credential without system interaction")
        let credential: String
        do {
            let value=try await readAISecret(KeychainAISecrets(allowInteraction:false),account:selected.secretAccount(),timeout:10)
            credential = try XCTUnwrap(value, "Configured credential missing.")
        } catch {
            throw XCTSkip("The separately signed XCTest host cannot read this credential without a system prompt. Use the already authorized native application for live acceptance.")
        }
        print("STRATA_LIVE credential available; no credential data is logged")
        let sample = try XCTUnwrap(Bundle.main.url(forResource: "sample-photo", withExtension: "jpg"))
        let output = ProcessInfo.processInfo.environment["STRATA_LIVE_AI_REPORT"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.temporaryDirectory.appendingPathComponent("Strata-AI-live-\(UUID().uuidString).json")
        var cases: [AIJSON] = []
        let taskNames=ProcessInfo.processInfo.environment["STRATA_LIVE_TASKS"]?.split(separator:",").map(String.init) ?? ["create","inspect","export","question","stop"]
        for provider in [AIProvider.openAi, .anthropic] {
            var config = selected; config.kind = provider
            let modelKey=provider == .anthropic ? "STRATA_LIVE_ANTHROPIC_MODEL" : "STRATA_LIVE_OPENAI_MODEL"
            if let model=ProcessInfo.processInfo.environment[modelKey], !model.isEmpty {config.model=model}
            if ProcessInfo.processInfo.environment["STRATA_LIVE_PROBE_CAPABILITIES"] == "1" {
                let modelWire=try AIWireRequest.decode(aiModelsRequest(configJson:config.json()))
                let modelData=try await AITransport().collect(modelWire,key:credential)
                let models=(try AIJSON.decode(String(decoding:modelData,as:UTF8.self)))["data"].array.compactMap{$0["id"].string}
                let visionWire=try AIWireRequest.decode(aiProbeRequest(configJson:config.json(),vision:true))
                let visionData=try await AITransport().collect(visionWire,key:credential)
                let supported=(try? aiProbeResult(configJson:config.json(),response:visionData,vision:true)) ?? false
                config.vision=supported
                cases.append(.object(["task":.string("capabilities"),"provider":.string(provider.rawValue),"models":.array(models.map(AIJSON.string)),"vision":.bool(supported),"raw_synthetic_probe":.string(String(decoding:visionData,as:UTF8.self))]))
            }
            // Same endpoint and credential are used with each protocol only
            // for the test fixture explicitly authorized by the user.
            for task in taskNames {
                print("STRATA_LIVE starting \(provider.rawValue) \(task)")
                let result = try await exercise(config: config, credential: credential, sample: sample, task: task)
                cases.append(result)
                print("STRATA_LIVE completed \(provider.rawValue) \(task) passed=\(result["passed"].bool)")
                try AIJSON.object(["cases": .array(cases), "status": .string("in_progress")]).encoded().data(using: .utf8)?.write(to: output, options: .atomic)
            }
        }
        try AIJSON.object(["cases": .array(cases), "status": .string("completed"), "fixture": .string("repository sample-photo.jpg")]).encoded().data(using: .utf8)?.write(to: output, options: .atomic)
        XCTAssertGreaterThanOrEqual(cases.count, taskNames.count*2)
    }

    @MainActor
    private func exercise(config: AIConfiguration, credential: String, sample: URL, task: String) async throws -> AIJSON {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        _ = try engine.openImage(path: sample.path)
        let local=task.hasPrefix("local_")
        if task=="local_16" {
            _ = try engine.dispatch(command:"file.new",paramsJson:commandJSON(["width":1024,"height":1536,"bit_depth":16,"discard_changes":true]))
            let read=try engine.grantRead(path:sample.path)
            _ = try engine.dispatch(command:"file.place",paramsJson:commandJSON(["resource":read]))
        }
        if local {try SubjectModelStore.shared.configure(engine)}
        if ["export", "question", "stop"].contains(task) { _ = try engine.dispatch(command: "layer.create", paramsJson: commandJSON(["name": "草稿"])) }
        if task=="groups"{for name in ["A","B","C"]{_ = try engine.dispatch(command:"layer.add_adjustment",paramsJson:commandJSON(["name":name,"type":"exposure"]))}}
        if task=="clip"{_ = try engine.dispatch(command:"layer.duplicate",paramsJson:commandJSON(["layer":["name":"sample-photo"]]))}
        if task=="stamp"{_ = try engine.dispatch(command:"layer.add_adjustment",paramsJson:commandJSON(["name":"提亮","type":"exposure","stops":0.5]))}
        let baseline = try EditorSnapshot.decode(engine.snapshotJson())
        let prompts = [
            "create": "新建一个图层叫‘草稿’，不透明度 50%。",
            "inspect": "这张图有哪些图层？请先使用 inspect_document 读取真实状态，然后简短回答。",
            "export": "把最上面的‘草稿’图层混合模式改成滤色，然后导出 PNG。通过宿主文件授权选择目标。",
            "question": "你觉得要不要保留‘草稿’图层？请通过 ask_user 向我提问并给出保留/删除选项，在我回答之前不修改文档。",
            "stop": "先删掉‘草稿’图层。"
            ,"groups":"把 A、B、C 三个相邻的调整图层放进一个组，叫‘调色’，保持原有顺序。"
            ,"stroke":"给 sample-photo 像素图层添加白色描边，2 像素，位于外部；保持原始图层和像素不变。"
            ,"clip":"让最上面的 sample-photo copy 图层只作用于下面的 sample-photo 图片，创建剪贴蒙版。"
            ,"stamp":"把所有可见图层合并成一张新的像素层，但保留原来的所有图层。请使用盖印。"
        ]
        let localPrompt="人物提亮半档，背景不要动。先读取真实文档与画布，通过选择主体创建带当前选区蒙版的曝光调整图层，曝光 +0.5 档。然后使用 ask_user 暂停，让我指定区域修正蒙版；此时先不要结束。"
        let options = try commandJSON(["mode": "ask_first", "max_steps": 50, "request_timeout_seconds": 60, "turn_timeout_seconds": 300])
        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("Strata-live-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: temporary, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporary) }
        let exported = temporary.appendingPathComponent("fixture.png")
        let beforeFile=temporary.appendingPathComponent("before.png")
        if local {try exportPNG(engine,to:beforeFile)}
        let turn = try engine.aiBegin(configJson: config.json(), optionsJson: options, prompt: local ? localPrompt : prompts[task] ?? "", shareCanvas: config.vision, priorJson: "[]")
        defer { try? engine.aiStop(turnId: turn) }
        var steps: [AIJSON] = []; var pausedQuestion = false; var stopped = false; var finished = false
        var finalStatus = ""
        for _ in 0..<150 {
            let raw = try await aiEngineWork { try engine.aiPoll(turnId: turn) }
            let step = try AIJSON.decode(raw); steps.append(stepWithoutRequestBody(step))
            switch step["kind"].string {
            case "request":
                print("STRATA_LIVE request \(config.kind.rawValue) \(task)")
                let request = try AIWireRequest.decode(step["request"].encoded())
                let requestID = try XCTUnwrap(step["request_id"].string)
                try await AITransport().send(request, key: credential) { data in
                    _ = try await aiEngineWork { try engine.aiFeed(requestId: requestID, bytes: data) }
                }
                _ = try await aiEngineWork { try engine.aiFinishResponse(requestId: requestID) }
                print("STRATA_LIVE response \(config.kind.rawValue) \(task)")
            case "paused":
                if task == "stop" {
                    try engine.aiStop(turnId: turn); stopped = true; finished = true; break
                }
                let question = try XCTUnwrap(step["question_id"].string)
                var answer: [String: Any] = ["question_id": question, "approved": true, "answer": "保留"]
                if step["reason"].string == "file_resource" {
                    answer["resource"] = try engine.aiGrantWrite(turnId: turn, path: exported.path, overwrite: false)
                }
                if step["reason"].string == "question" {
                    pausedQuestion = true
                    if local {
                        _ = try engine.dispatch(command:"select.marquee",paramsJson:commandJSON(["x":280,"y":650,"width":160,"height":240]))
                        answer["answer"]="按我暂停时新框选的矩形区域来：替换刚才曝光层的蒙版为当前选区，曝光保持 +0.5 档。保留原始图层，完成后直接总结，不用再次询问。"
                    }
                }
                let encoded = try commandJSON(answer)
                let response = try await aiEngineWork { try engine.aiRespond(answerJson: encoded) }
                steps.append(try AIJSON.decode(response))
            case "finished": finished = true; finalStatus=step["status"].string ?? ""
            default: break
            }
            if finished { break }
        }
        XCTAssertTrue(finished, "Model must finish within bounded steps.")
        let final = try EditorSnapshot.decode(engine.snapshotJson())
        XCTAssertEqual(final.control?.kind, "user")
        var passed = finished && final.control?.kind == "user" && (task=="stop" || finalStatus=="completed")
        var metrics:AIJSON = .null
        switch task {
        case "create":
            XCTAssertEqual(final.layers.last?.name, "草稿"); XCTAssertEqual(final.layers.last?.opacity, 0.5)
            passed = passed && final.layers.last?.name == "草稿" && final.layers.last?.opacity == 0.5
            _ = try engine.dispatch(command: "edit.undo_turn", paramsJson: commandJSON(["turn_id": turn]))
            XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).layers.count, baseline.layers.count)
        case "inspect": XCTAssertEqual(final.revision, baseline.revision); passed = passed && final.revision == baseline.revision
        case "export":
            XCTAssertEqual(final.layers.last?.blendMode, "screen"); XCTAssertTrue(FileManager.default.fileExists(atPath: exported.path))
            passed = passed && final.layers.last?.blendMode == "screen" && FileManager.default.fileExists(atPath: exported.path)
        case "question":
            XCTAssertTrue(pausedQuestion); XCTAssertEqual(final.revision, baseline.revision)
            passed = passed && pausedQuestion && final.revision == baseline.revision
        case "stop":
            XCTAssertTrue(stopped); XCTAssertEqual(final.revision, baseline.revision)
            passed = passed && stopped && final.revision == baseline.revision
        case "groups":
            let group=final.layers.first{$0.isGroup&&$0.name=="调色"}
            let children=final.layers.filter{["A","B","C"].contains($0.name)}
            let valid=group != nil && children.count==3 && children.allSatisfy{$0.parent==group?.id} && final.layers.count==baseline.layers.count+1
            XCTAssertTrue(valid);passed = passed&&valid
        case "stroke":
            let stroke=final.layers.first{$0.name=="sample-photo"}?.styles?["stroke"]
            let valid=stroke?["size"].number==2 && stroke?["position"].string=="outside" && stroke?["color"].array.compactMap(\.number)==[255,255,255,255] && final.layers.count==baseline.layers.count
            XCTAssertTrue(valid);passed=passed&&valid
        case "clip":
            let valid=final.layers.last?.clipped==true && final.layers.count==baseline.layers.count
            XCTAssertTrue(valid);passed=passed&&valid
        case "stamp":
            let valid=final.layers.count==baseline.layers.count+1 && Set(baseline.layers.map(\.id)).isSubset(of:Set(final.layers.map(\.id))) && final.history.last?.command=="layer.stamp_visible"
            XCTAssertTrue(valid);passed=passed&&valid
        case "local_8","local_16":
            XCTAssertTrue(pausedQuestion)
            XCTAssertEqual(final.layers.count,baseline.layers.count+1)
            XCTAssertEqual(final.layers.last?.exposure,0.5)
            _ = try engine.dispatch(command:"edit.undo_turn",paramsJson:commandJSON(["turn_id":turn]))
            XCTAssertEqual(try EditorSnapshot.decode(engine.snapshotJson()).layers.count,baseline.layers.count)
            while true {
                let history=try EditorSnapshot.decode(engine.snapshotJson())
                if history.historyCursor>=history.history.count {break}
                _ = try engine.dispatch(command:"edit.redo",paramsJson:"{}")
            }
            let project=temporary.appendingPathComponent("local.strata")
            let write=try engine.grantWrite(path:project.path,overwrite:false)
            _ = try engine.dispatch(command:"file.save_as",paramsJson:commandJSON(["resource":write]))
            let read=try engine.grantRead(path:project.path)
            _ = try engine.dispatch(command:"file.open",paramsJson:commandJSON(["resource":read]))
            try exportPNG(engine,to:exported)
            let before=try pixelSamples(beforeFile);let after=try pixelSamples(exported)
            let targetRatio=after[0].reduce(0,+)/max(0.0001,before[0].reduce(0,+))
            let backgroundError=zip(before.dropFirst(),after.dropFirst()).flatMap{a,b in zip(a,b).map{abs($0-$1)}}.max() ?? 0
            let tolerance=3.0/(task=="local_16" ? 65535.0 : 255.0)
            XCTAssertGreaterThan(targetRatio,1.05);XCTAssertLessThanOrEqual(backgroundError,tolerance)
            passed = passed && pausedQuestion && final.layers.count==baseline.layers.count+1 && final.layers.last?.exposure==0.5 && targetRatio>1.05 && backgroundError<=tolerance
            metrics = .object(["target_brightness_ratio":.number(targetRatio),"protected_max_channel_error":.number(backgroundError),"protected_tolerance":.number(tolerance),"correction":.object(["x":.number(280),"y":.number(650),"width":.number(160),"height":.number(240)])])
        default: break
        }
        let transcript = try AIJSON.decode(engine.aiTranscriptJson(turnId: turn))
        return .object(["task": .string(task), "provider": .string(config.kind.rawValue), "model": .string(config.model),
                        "completed": .bool(finished), "passed": .bool(passed), "question": .bool(pausedQuestion), "stopped": .bool(stopped),
                        "steps": .array(steps), "transcript": transcript,"metrics":metrics])
    }
    private func stepWithoutRequestBody(_ step: AIJSON) -> AIJSON {
        guard case .object(var fields) = step else { return step }
        fields.removeValue(forKey: "request"); return .object(fields)
    }
    private func exportPNG(_ engine:StrataEngine,to path:URL)throws{
        let resource=try engine.grantWrite(path:path.path,overwrite:false)
        _ = try engine.dispatch(command:"file.export",paramsJson:commandJSON(["resource":resource,"format":"png"]))
    }
    private func pixelSamples(_ path:URL)throws->[[Double]]{
        let bitmap=try XCTUnwrap(NSBitmapImageRep(data:Data(contentsOf:path)))
        return try [(300,800),(50,50),(350,1050)].map{x,y in
            let color=try XCTUnwrap(bitmap.colorAt(x:x,y:y)?.usingColorSpace(.sRGB))
            return [color.redComponent,color.greenComponent,color.blueComponent].map(Double.init)
        }
    }
}
