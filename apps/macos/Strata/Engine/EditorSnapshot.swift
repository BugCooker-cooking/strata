import Foundation

struct EditorSnapshot: Decodable, Sendable {
    struct Control: Decodable, Sendable { let kind: String }
    struct Frame: Decodable, Sendable { let name: String; let width: UInt32; let height: UInt32 }
    struct Layer: Decodable, Sendable { let id: String; let name: String; let visible: Bool; let opacity: Double; let blendMode: String; let x: Int64; let y: Int64 }
    struct History: Decodable, Sendable { let command: String; let source: String }
    struct Execution: Decodable, Sendable { let command: String; let status: String }
    let revision: UInt64
    let dirty: Bool
    let frame: Frame?
    let bitDepth: UInt8?
    let profileName: String?
    let profileFallback: Bool
    let layers: [Layer]
    let activeLayer: String?
    let history: [History]
    let historyCursor: Int
    let executions: [Execution]
    let control: Control?

    static func decode(_ text: String) throws -> EditorSnapshot {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(EditorSnapshot.self, from: Data(text.utf8))
    }
}

func commandJSON(_ params: [String: Any]) throws -> String {
    String(decoding: try JSONSerialization.data(withJSONObject: params, options: [.sortedKeys]), as: UTF8.self)
}
