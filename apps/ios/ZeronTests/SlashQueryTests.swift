import XCTest
@testable import Zeron

final class SlashQueryTests: XCTestCase {
    func testOnlyLeadingSlashOpensMenu() {
        for text in ["    /goal", "\t/goal", "ask /goal", "/goal ship it", "```\n/goal", "/goal\t", "/goal\r", "/goal\u{00a0}"] {
            XCTAssertNil(SlashQuery.active(in: text, selection: NSRange(location: (text as NSString).length, length: 0)), text)
        }
        XCTAssertEqual(SlashQuery.active(in: "  /go", selection: NSRange(location: 5, length: 0))?.query, "go")
    }

    func testMidTokenCompletionReplacesWholeTokenAndKeepsArguments() {
        let text = "/gozzz ship it"
        let query = SlashQuery.active(in: text, selection: NSRange(location: 3, length: 0))!
        XCTAssertEqual(query.query, "go")
        XCTAssertEqual((text as NSString).replacingCharacters(in: query.range, with: "/goal"), "/goal ship it")
    }

    func testSelectionAndUnicodeCursor() {
        XCTAssertNil(SlashQuery.active(in: "/goal", selection: NSRange(location: 0, length: 5)))
        let text = "/审查"
        XCTAssertEqual(SlashQuery.active(in: text, selection: NSRange(location: (text as NSString).length, length: 0))?.query, "审查")
    }
}
