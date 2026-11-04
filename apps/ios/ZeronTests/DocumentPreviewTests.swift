import XCTest
import UIKit
@testable import Zeron

@MainActor
final class DocumentPreviewTests: XCTestCase {
    func testFirstDocumentFrameStartsAtTopWhileChatStartsAtTail() async throws {
        let relay = FrameRelay {}
        let engine = TranscriptView(text: TextEngine.shared, listener: relay)
        defer { engine.close() }
        let document = DocumentPreview(
            path: "docs/guide.md",
            text: (1...80).map { "## Section \($0)\n\nDocument content." }.joined(separator: "\n\n"),
            markdown: true, truncated: false, deviceId: "host", cwd: "/repo")
        engine.setViewport(width: 390, textScale: 1)
        engine.setDocument(chatId: "chat", document: document, markdown: true)
        let deadline = Date().addingTimeInterval(5)
        while engine.frame().rowCount() == 0 && Date() < deadline {
            try await Task.sleep(for: .milliseconds(20))
        }
        let frame = engine.frame()
        XCTAssertGreaterThan(frame.totalHeight(), 800)
        let reader = TranscriptListView(engine: engine, startsAtTop: true)
        reader.frame = CGRect(x: 0, y: 0, width: 390, height: 500)
        reader.apply(frame)
        XCTAssertEqual(reader.contentOffset.y, -reader.adjustedContentInset.top, accuracy: 1)
        XCTAssertFalse(reader.following)
        // A conversation that isn't following still opens at its tail.
        let chat = TranscriptListView(engine: engine)
        chat.frame = reader.frame
        chat.releaseFollow()
        chat.apply(frame)
        XCTAssertGreaterThan(chat.contentOffset.y, 0)
    }

    func testSharedLinkGrammarMatchesTheRawAndEncodedSpellings() {
        // Android hands over the raw Markdown target; URL(string:) encodes a literal `%`.
        XCTAssertEqual(documentLinkPath(url: "100%.md", basePath: nil), "100%.md")
        XCTAssertEqual(documentLinkPath(url: "100%25.md", basePath: nil), "100%.md")
        XCTAssertEqual(documentLinkPath(url: "guide.md#usage", basePath: "docs/guide.md"), "docs/guide.md")
        XCTAssertNil(documentLinkPath(url: "../../.ssh/id_ed25519", basePath: "docs/guide.md"))
    }
}
