import Foundation

enum InterfaceLanguage: String, CaseIterable {
    case chinese = "zh-Hans"
    case english = "en"

    var name: String {
        switch self {
        case .chinese: "简体中文"
        case .english: "English"
        }
    }
}

enum L10n {
    static let preferenceKey = "StrataInterfaceLanguage"
    static let language = preferredLanguage()

    static func preferredLanguage(in defaults: UserDefaults = .standard) -> InterfaceLanguage {
        defaults.string(forKey: preferenceKey).flatMap(InterfaceLanguage.init(rawValue:)) ?? .chinese
    }

    static func setPreferredLanguage(_ language: InterfaceLanguage, in defaults: UserDefaults = .standard) {
        defaults.set(language.rawValue, forKey: preferenceKey)
    }

    private static let english: [String: String] = {
        guard let url = Bundle.main.url(forResource: "English", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let catalog = try? JSONDecoder().decode([String: String].self, from: data)
        else { return [:] }
        return catalog
    }()

    static func text(_ source: String, language: InterfaceLanguage = language) -> String {
        language == .english ? english[source] ?? source : source
    }

    static func format(_ source: String, _ arguments: String..., language: InterfaceLanguage = language) -> String {
        let template = text(source, language: language)
        var result = ""
        var cursor = template.startIndex
        for match in template.matches(of: /\{([0-9]+)\}/) {
            result += template[cursor..<match.range.lowerBound]
            if let index = Int(match.1), arguments.indices.contains(index) {
                result += arguments[index]
            } else {
                result += template[match.range]
            }
            cursor = match.range.upperBound
        }
        result += template[cursor...]
        return result
    }
}
