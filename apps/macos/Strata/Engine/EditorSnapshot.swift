import Foundation

struct EditorSnapshot: Decodable, Sendable {
    struct PaintColors:Decodable,Sendable{let foreground:[Double];let background:[Double];let version:UInt64}
    let paintColors:PaintColors?
    struct Control: Decodable, Sendable { let kind: String; let turnId: String? }
    struct Frame: Decodable, Sendable { let name: String; let width: UInt32; let height: UInt32; let x: Int64; let y: Int64 }
    struct Layer: Decodable, Sendable {
        struct Kind: Decodable, Sendable {
            let kind:String;let stops:Double?
            let color:[UInt16]?;let start:[UInt16]?;let end:[UInt16]?;let angle:Double?;let scale:Double?;let radial:Bool?
            let hue:Double?;let saturation:Double?;let lightness:Double?
            let adjustment:AIJSON?
            let text:AIJSON?
        }
        struct Mask: Decodable, Sendable { let enabled: Bool }
        struct Locks: Decodable, Sendable { let transparency:Bool;let pixels:Bool;let position:Bool;let all:Bool }
        let id: String; let name: String; let visible: Bool; let opacity: Double; let blendMode: String; let x: Int64; let y: Int64
        let kind: Kind?; let mask: Mask?
        let fillOpacity:Double?;let locks:Locks?;let colorLabel:String?
        let parent:String?
        let clipped:Bool?
        let passThrough:Bool?
        let styles:AIJSON?
        var exposure: Double? { kind?.kind == "exposure" ? kind?.stops : nil }
        var isGroup:Bool{kind?.kind=="group"}
        var isAdjustment:Bool{["exposure","hue_saturation","adjustment"].contains(kind?.kind ?? "")}
        var title:String{
            switch kind?.kind{case "text":"可编辑文字";case "group":"图层组";case "exposure":"曝光调整图层";case "solid":"纯色填充";case "gradient":"渐变填充";case "hue_saturation":"色相/饱和度调整";case "adjustment":tonalTitle(kind?.adjustment?["type"].string ?? "");default:"像素图层"}
        }
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
