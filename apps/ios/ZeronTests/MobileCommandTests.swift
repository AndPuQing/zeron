import XCTest
import UIKit
@testable import Zeron

final class MobileCommandTests: XCTestCase {
    private var context: MobileComposerContext {
        MobileComposerContext(inSession: true, isChild: false, canFork: true, hasReply: true, busy: false, editing: false)
    }
    private var modelCommand: ComposerCommand {
        ComposerCommand(token: "/model", description: "Model", inputHint: nil, insertion: "/model", unavailableReason: "Desktop only")
    }

    @MainActor
    func testSelectionExecutesLocallyAndPreservesRestOfDraft() async {
        let composer = ComposerBar()
        composer.commandHarness = { "codex" }
        composer.localCommandContext = { self.context }
        let invoked = expectation(description: "local action")
        composer.onLocalCommand = { action in
            XCTAssertEqual(action, .model)
            invoked.fulfill()
        }
        composer.onSend = { _, _, _ in XCTFail("Local action must not send a prompt"); return .kept }
        composer.text = "/mo keep this"
        composer.textView.selectedRange = NSRange(location: 3, length: 0)
        composer.insertCommand(modelCommand)
        XCTAssertEqual(composer.text, " keep this")
        XCTAssertEqual(composer.encodedDraft, " keep this")
        await fulfillment(of: [invoked], timeout: 2)
    }

    @MainActor
    func testDirectSubmissionDoesNotSendLocalActionsOrTheirArguments() async {
        let composer = ComposerBar()
        composer.commandHarness = { "codex" }
        composer.localCommandContext = { self.context }
        var actions = 0
        let invoked = expectation(description: "direct action")
        composer.onLocalCommand = { _ in actions += 1; invoked.fulfill() }
        composer.onSend = { _, _, _ in XCTFail("Local action must not reach send"); return .kept }
        func sendButton(_ view: UIView) -> UIButton? {
            if view.accessibilityIdentifier == "composer-send" { return view as? UIButton }
            return view.subviews.compactMap(sendButton).first
        }
        composer.text = "/model extra"
        let send = sendButton(composer)!
        send.sendActions(for: .touchUpInside)
        XCTAssertEqual(actions, 0)
        XCTAssertEqual(composer.text, "/model extra")
        composer.text = "/model"
        send.sendActions(for: .touchUpInside)
        await fulfillment(of: [invoked], timeout: 2)
        XCTAssertEqual(actions, 1)
        XCTAssertEqual(composer.text, "")
    }

    @MainActor
    func testRejectedForkPreservesDraft() {
        let composer = ComposerBar()
        composer.commandHarness = { "codex" }
        composer.localCommandContext = {
            var context = self.context
            context.canFork = false
            return context
        }
        composer.onLocalCommand = { _ in XCTFail("Fork is unavailable") }
        composer.text = "/fork"
        composer.textView.selectedRange = NSRange(location: 5, length: 0)
        composer.insertCommand(ComposerCommand(token: "/fork", description: "Fork", inputHint: nil, insertion: "/fork", unavailableReason: "Desktop only"))
        XCTAssertEqual(composer.text, "/fork")
    }

    @MainActor
    func testFailedLocalActionRestoresCommand() async {
        let composer = ComposerBar()
        composer.commandHarness = { "codex" }
        composer.localCommandContext = { self.context }
        composer.onLocalCommand = { _ in throw NSError(domain: "test", code: 1) }
        composer.text = "/model"
        composer.textView.selectedRange = NSRange(location: 6, length: 0)
        composer.insertCommand(modelCommand)
        for _ in 0..<10 { await Task.yield() }
        XCTAssertEqual(composer.text, "/model")
    }

    @MainActor
    func testOtherAgentCommandKeepsItsOriginalDisabledState() {
        let composer = ComposerBar()
        composer.commandHarness = { "claude-code" }
        composer.localCommandContext = { self.context }
        composer.onLocalCommand = { _ in XCTFail("Must not intercept another agent") }
        composer.text = "/model"
        composer.textView.selectedRange = NSRange(location: 6, length: 0)
        composer.insertCommand(modelCommand)
        XCTAssertEqual(composer.text, "/model")
    }
}
