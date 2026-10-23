import XCTest
import UIKit
@testable import Zeron

final class ComposerSelectionTests: XCTestCase {
    private func skillLink(_ path: String) -> String {
        let payload = "{\"kind\":\"skill\",\"name\":\"review\",\"path\":\"\(path)\"}".utf8.map { String(format: "%02x", $0) }.joined()
        return "[$review](zeron-invoke:\(payload))"
    }

    private var wire: String { skillLink("/b/SKILL.md") + " check this" }

    @MainActor
    func testInsertingSameNameSkillAfterRestorePreservesBothIdentities() {
        let composer = ComposerBar()
        composer.text = wire
        composer.textView.text = "$ " + composer.text
        composer.textView.selectedRange = NSRange(location: 1, length: 0)
        composer.insertCommand(ComposerCommand(
            token: "$review", description: "", inputHint: nil,
            insertion: skillLink("/a/SKILL.md"), unavailableReason: nil
        ))
        XCTAssertEqual(composer.text, "$review:2 $review check this")
        XCTAssertEqual(composer.encodedDraft, skillLink("/a/SKILL.md") + " " + wire)
        let reopened = ComposerBar()
        reopened.text = composer.persistedDraft
        XCTAssertEqual(reopened.encodedDraft, composer.encodedDraft)
    }

    @MainActor
    func testContextChangeBlocksSendingAndSurvivesDraftRestore() {
        let composer = ComposerBar()
        composer.commandContext = "host:agent:project-a"
        composer.text = wire
        XCTAssertNil(composer.selectionError)
        composer.commandContext = "host:agent:project-b"
        XCTAssertNotNil(composer.selectionError)
        var sent = false
        composer.onSend = { _, _, _ in sent = true; return .sent }
        func findSend(_ view: UIView) -> UIButton? {
            if view.accessibilityIdentifier == "composer-send" { return view as? UIButton }
            return view.subviews.compactMap(findSend).first
        }
        let send = findSend(composer)!
        XCTAssertFalse(send.isEnabled)
        send.sendActions(for: .touchUpInside)
        XCTAssertFalse(sent)
        let reopened = ComposerBar()
        reopened.text = composer.persistedDraft
        XCTAssertNotNil(reopened.selectionError)
        reopened.textView.text = "check this"
        reopened.textViewDidChange(reopened.textView)
        XCTAssertNil(reopened.selectionError)
    }

    @MainActor
    func testPrependingTextKeepsSkillIdentity() {
        let composer = ComposerBar()
        composer.text = wire
        composer.textView.text = "Please use " + composer.textView.text
        XCTAssertTrue(composer.encodedDraft.contains("zeron-invoke:"))
        let reopened = ComposerBar()
        reopened.text = composer.persistedDraft
        XCTAssertEqual(reopened.encodedDraft, composer.encodedDraft)
    }
}
