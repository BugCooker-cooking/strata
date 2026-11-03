import Foundation

/// UI history only. A selected entry still requires a fresh host read grant.
struct RecentFileStore {
    private let defaults: UserDefaults
    private let key = "workspace.recentFiles.v1"
    init(defaults: UserDefaults = .standard) { self.defaults = defaults }
    var urls: [URL] { (defaults.stringArray(forKey: key) ?? []).map { URL(fileURLWithPath: $0) } }
    func record(_ url: URL) {
        let normalized = url.standardizedFileURL
        var values = urls.filter { $0.standardizedFileURL != normalized }
        values.insert(normalized, at: 0)
        defaults.set(Array(values.prefix(10)).map(\.path), forKey: key)
    }
    func clear() { defaults.removeObject(forKey: key) }
}
