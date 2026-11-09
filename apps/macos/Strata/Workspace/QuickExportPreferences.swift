import Foundation
struct ExportPresetSnapshot:Decodable{let version:UInt64;let options:AIJSON}

/// Only encoder options persist; paths, tokens and overwrite approval never do.
@MainActor
struct QuickExportPreferences{
    let defaults:UserDefaults
    static let key="export.quick.options.v1"
    static var restoreError=""
    init(defaults:UserDefaults = .standard){self.defaults=defaults}
    func restore(on engine:StrataEngine)throws{
        guard let saved=defaults.string(forKey:Self.key)else{return}
        guard saved.utf8.count<=8192 else{throw QuickExportError("快速导出预设过大，请重新设置。")}
        let options=try AIJSON.decode(saved),current=try preset(engine)
        _ = try engine.dispatch(command:"file.export_preset.set",paramsJson:commandJSON(["options":JSONSerialization.jsonObject(with:Data(options.encoded().utf8)),"expected_version":current.version]))
    }
    func save(from engine:StrataEngine)throws{
        let value=try preset(engine).options.encoded();defaults.set(value,forKey:Self.key)
    }
    func preset(_ engine:StrataEngine)throws->ExportPresetSnapshot{try JSONDecoder().decode(ExportPresetSnapshot.self,from:Data(engine.dispatch(command:"file.export_preset.get",paramsJson:"{}").utf8))}
    func format(_ preset:ExportPresetSnapshot)throws->String{
        guard let value=preset.options["format"].string,["png","tiff","jpeg","webp"].contains(value)else{throw QuickExportError("快速导出格式无效，请重新设置预设。")};return value
    }
}
struct QuickExportError:LocalizedError{
    let message:String
    init(_ message:String){self.message=message}
    var errorDescription:String?{message}
}
