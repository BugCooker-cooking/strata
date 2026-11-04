import Foundation

struct EditorSnapshot: Decodable, Sendable {
    struct Control: Decodable, Sendable { let kind: String; let turnId: String? }
    struct Frame: Decodable, Sendable { let name: String; let width: UInt32; let height: UInt32; let x: Int64; let y: Int64 }
    struct Layer: Decodable, Sendable {
        struct Kind: Decodable, Sendable { let kind: String; let stops: Double? }
        struct Mask: Decodable, Sendable { let enabled: Bool }
        let id: String; let name: String; let visible: Bool; let opacity: Double; let blendMode: String; let x: Int64; let y: Int64
        let kind: Kind?; let mask: Mask?
        var exposure: Double? { kind?.kind == "exposure" ? kind?.stops : nil }
        var title: String { exposure != nil ? "曝光调整图层" : "像素图层" }
    }
    struct History: Decodable, Sendable { let command: String; let source: String; let turnId: String? }
    struct Execution: Decodable, Sendable { let command: String; let status: String }
    let revision: UInt64
    let sessionId: String?
    let documentId: String?
    let dirty: Bool
    let frame: Frame?
    let bitDepth: UInt8?
    let resolutionDpi: UInt32?
    let preset: String?
    let profileName: String?
    let profileFallback: Bool
    let layers: [Layer]
    let activeLayer: String?
    let history: [History]
    let historyCursor: Int
    let executions: [Execution]
    let control: Control?
    let selection: AIJSON?
    let selectionExtent: AIJSON?

    static func decode(_ text: String) throws -> EditorSnapshot {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(EditorSnapshot.self, from: Data(text.utf8))
    }
}

func commandJSON(_ params: [String: Any]) throws -> String {
    String(decoding: try JSONSerialization.data(withJSONObject: params, options: [.sortedKeys]), as: UTF8.self)
}
