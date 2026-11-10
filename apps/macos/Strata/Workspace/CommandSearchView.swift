import SwiftUI

struct CommandSearchMatch: Decodable, Identifiable {
    let id: String
    let title: String
    let summary: String
}

enum CommandSearchCatalog {
    private struct Results: Decodable {
        let matches: [CommandSearchMatch]
    }

    static func search(_ input: String) throws -> [CommandSearchMatch] {
        let query = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return [] }
        let json = try searchCommandCatalog(query: query)
        return try JSONDecoder().decode(Results.self, from: Data(json.utf8)).matches
    }

    static func description(for id: String) throws -> String {
        let result = try AIJSON.decode(describeCatalogCommand(id: id))
        return result["commands"].array.first?["description"].string ?? ""
    }
}

struct CommandSearchView: View {
    let canDraft: Bool
    let addToDraft: (CommandSearchMatch) -> Void
    @FocusState private var searchFocused: Bool
    @State private var input = ""
    @State private var results: [CommandSearchMatch] = []
    @State private var selectedID: String?
    @State private var description = ""
    @State private var errorMessage = ""

    private var selected: CommandSearchMatch? {
        results.first { $0.id == selectedID }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("输入名称或关键词，如裁剪、快速导出", text: $input)
                .textFieldStyle(.roundedBorder)
                .accessibilityLabel("搜索功能")
                .focused($searchFocused)
            if results.isEmpty {
                ContentUnavailableView(
                    input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "搜索 Strata 功能" : "没有找到功能",
                    systemImage: "magnifyingglass",
                    description: Text(errorMessage.isEmpty ? "可使用中文、英文或命令名称搜索。" : errorMessage)
                )
            } else {
                HStack(spacing: 12) {
                    List(results, selection: $selectedID) { match in
                        VStack(alignment: .leading, spacing: 3) {
                            Text(match.title)
                            Text(match.summary).font(.caption).foregroundStyle(.secondary)
                        }
                        .tag(match.id)
                    }
                    .frame(width: 235)
                    Divider()
                    VStack(alignment: .leading, spacing: 8) {
                        if let selected {
                            Text(selected.title).font(.headline)
                            ScrollView {
                                Text(description.isEmpty ? selected.summary : description)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            if canDraft {
                                Button("带入 AI 草稿") { addToDraft(selected) }
                                    .help("将这项功能带到聊天输入框，供你继续编辑")
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .padding(8)
        .frame(width: 630, height: 380)
        .onAppear { searchFocused = true }
        .onChange(of: input) { _, query in updateResults(query) }
        .onChange(of: selectedID) { _, id in
            description = id.flatMap { try? CommandSearchCatalog.description(for: $0) } ?? ""
        }
    }

    private func updateResults(_ query: String) {
        do {
            results = try CommandSearchCatalog.search(query)
            errorMessage = ""
        } catch {
            results = []
            errorMessage = query.utf8.count > 256 ? "关键词过长，请缩短后重试。" : "暂时无法搜索命令目录。"
        }
        if selected == nil { selectedID = results.first?.id }
    }
}
