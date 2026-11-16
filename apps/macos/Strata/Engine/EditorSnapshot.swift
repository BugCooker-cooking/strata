import Foundation

struct EditorSnapshot: Decodable, Sendable {
    struct Guide:Decodable,Sendable,Equatable{let id:String;let name:String;let axis:String;let position:Double}
    struct Presentation:Decodable,Sendable,Equatable{
        let rulerUnit:String?;let rulerOrigin:[Double]?
        let rulersVisible:Bool;let guidesVisible:Bool;let guidesLocked:Bool;let snapEnabled:Bool;let snapTolerance:Double;let snapArtboard:Bool;let snapLayers:Bool?;let version:UInt64
        static let defaults=Presentation(rulerUnit:"pixels",rulerOrigin:[0,0],rulersVisible:true,guidesVisible:true,guidesLocked:false,snapEnabled:false,snapTolerance:6,snapArtboard:true,snapLayers:false,version:0)
    }
    struct RulerMetrics:Decodable,Sendable,Equatable{
        let unit:String;let origin:[Double];let pixelsPerUnit:[Double];let resolutionDpi:UInt32;let presentationVersion:UInt64
        var valid:Bool{origin.count==2 && pixelsPerUnit.count==2 && origin.allSatisfy{$0.isFinite&&abs($0)<=1e12} && pixelsPerUnit.allSatisfy{$0.isFinite&&$0>0}}
        var symbol:String{switch unit{case "inches":"in";case "centimeters":"cm";case "millimeters":"mm";case "points":"pt";case "percent":"%";default:"px"}}
        func rulerPoint(_ canvas:NSPoint)->NSPoint?{guard valid else{return nil};return NSPoint(x:(Double(canvas.x)-origin[0])/pixelsPerUnit[0],y:(Double(canvas.y)-origin[1])/pixelsPerUnit[1])}
    }
    let rulerMetrics:RulerMetrics?
    let guides:[Guide]?
    let presentation:Presentation?
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
        func canTransform(scope:String)->Bool{
            let eligible = scope == "whole_layer" ? !isAdjustment || mask != nil : scope == "selection" && (kind?.kind ?? "pixel") == "pixel"
            return eligible && locks?.all != true && locks?.pixels != true && locks?.position != true && locks?.transparency != true
        }
        var isGroup:Bool{kind?.kind=="group"}
        var isAdjustment:Bool{["exposure","hue_saturation","adjustment"].contains(kind?.kind ?? "")}
        var title:String{
            switch kind?.kind{case "text":L10n.text("可编辑文字");case "group":L10n.text("图层组");case "exposure":L10n.text("曝光调整图层");case "solid":L10n.text("纯色填充");case "gradient":L10n.text("渐变填充");case "hue_saturation":L10n.text("色相/饱和度调整");case "adjustment":tonalTitle(kind?.adjustment?["type"].string ?? "");default:L10n.text("像素图层")}
        }
    }
    struct History: Decodable, Sendable { let command: String; let source: String; let turnId: String? }
    struct Execution: Decodable, Sendable { let command: String; let status: String }
    let revision: UInt64
    let sessionId: String?
    let documentId: String?
    let dirty: Bool
    let frame: Frame?
    let colorMode: String?
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
