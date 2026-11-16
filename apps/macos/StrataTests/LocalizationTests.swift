import Foundation
import XCTest
@testable import Strata

final class LocalizationTests: XCTestCase {
    func testPackagedCatalogCoversMenusEditingErrorsAndApproval() {
        XCTAssertEqual(L10n.text("文件", language: .chinese), "文件")
        XCTAssertEqual(L10n.text("文件", language: .english), "File")
        XCTAssertEqual(L10n.text("自由变换", language: .english), "Free Transform")
        XCTAssertEqual(L10n.text("应用调整", language: .english), "Apply Adjustment")
        XCTAssertEqual(L10n.text("授权本轮文档上下文", language: .english), "Approve document context for this turn")
        XCTAssertEqual(L10n.text("操作已取消。", language: .english), "Operation canceled.")
        XCTAssertEqual(L10n.format("参考线 {0}", "Willow {1}", language: .english), "Guide Willow {1}")
    }

    func testLanguageSelectionPersistsWithoutChangingTheSystemLocale() throws {
        let suite = "dev.strata-editor.localization.\(UUID())"
        let isolated = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { isolated.removePersistentDomain(forName: suite) }
        XCTAssertEqual(L10n.preferredLanguage(in: isolated), .chinese)
        L10n.setPreferredLanguage(.english, in: isolated)
        let reloaded = try XCTUnwrap(UserDefaults(suiteName: suite))
        XCTAssertEqual(L10n.preferredLanguage(in: reloaded), .english)
        XCTAssertNil(reloaded.persistentDomain(forName: suite)?["AppleLanguages"])
    }
}
