import XCTest
@testable import Zeron

final class ComposerQueryTests: XCTestCase {
    func testCompletionUsesDesktopProseBoundaries() {
        for text in ["    /goal", "\t/goal", "/goal ship it", "```\n/goal", "/goal\t", "/goal\r", "/goal\u{00a0}"] {
            XCTAssertNil(ComposerQuery.active(in: text, selection: NSRange(location: (text as NSString).length, length: 0)), text)
        }
        XCTAssertEqual(ComposerQuery.active(in: "ask /goal", selection: NSRange(location: 9, length: 0))?.query, "goal")
        XCTAssertEqual(ComposerQuery.active(in: "  /go", selection: NSRange(location: 5, length: 0))?.query, "go")
    }

    func testMidTokenCompletionReplacesWholeTokenAndKeepsArguments() {
        let text = "/gozzz ship it"
        let query = ComposerQuery.active(in: text, selection: NSRange(location: 3, length: 0))!
        XCTAssertEqual(query.query, "go")
        XCTAssertEqual((text as NSString).replacingCharacters(in: query.range, with: "/goal"), "/goal ship it")
    }

    func testSelectionAndUnicodeCursor() {
        XCTAssertNil(ComposerQuery.active(in: "/goal", selection: NSRange(location: 0, length: 5)))
        let text = "/审查"
        XCTAssertEqual(ComposerQuery.active(in: text, selection: NSRange(location: (text as NSString).length, length: 0))?.query, "审查")
    }
    func testInlineSkillAndUtf16Replacement() {
        let text = "👋 使用 ($review) 然后继续"
        let cursor = ("👋 使用 ($re" as NSString).length
        let query = ComposerQuery.active(in: text, selection: NSRange(location: cursor, length: 0))!
        XCTAssertTrue(query.skills)
        XCTAssertEqual(query.query, "re")
        XCTAssertEqual((text as NSString).replacingCharacters(in: query.range, with: "$other"), "👋 使用 ($other) 然后继续")
        for text in ["$100", "`$review`", "\\$review", "[label]($review", "```\n$review", "$repo/file"] {
            XCTAssertNil(ComposerQuery.active(in: text, selection: NSRange(location: (text as NSString).length, length: 0)), text)
        }
    }
}
