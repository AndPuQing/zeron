import XCTest
import UIKit
@testable import Zeron

final class MobileCommandTests: XCTestCase {
    func testExecutionMenuSelectionKeepsNewerIndependentSetting() {
        var latest = ChatConfig(accessMode: "full-access", plan: false, harness: "codex", model: nil, reasoning: nil, modelOptions: [:], sandbox: .workspaceWrite)
        let accessSelection = ExecutionSelection.access("supervised")
        // Another device changes Plan after the access menu was opened.
        latest.plan = true
        accessSelection.apply(to: &latest)
        XCTAssertEqual(latest.accessMode, "supervised")
        XCTAssertTrue(latest.plan)
        let planSelection = ExecutionSelection.plan(false)
        latest.accessMode = "auto-accept-edits"
        planSelection.apply(to: &latest)
        XCTAssertEqual(latest.accessMode, "auto-accept-edits")
        XCTAssertFalse(latest.plan)
    }

    func testExecutionPolicySurvivesDraftRestoreAndOldDraftsDecode() throws {
        let old = try JSONDecoder().decode(NewSessionDraft.self, from: Data(#"{"harness":"codex","worktree":false}"#.utf8))
        XCTAssertNil(old.accessMode)
        XCTAssertNil(old.plan)
        var draft = old
        draft.accessMode = "supervised"
        draft.plan = true
        XCTAssertEqual(try JSONDecoder().decode(NewSessionDraft.self, from: JSONEncoder().encode(draft)), draft)
    }

    private var context: MobileComposerContext {
        MobileComposerContext(inSession: true, isChild: false, canFork: true, hasReply: true, busy: false, editing: false)
    }
    private var modelCommand: ComposerCommand {
        ComposerCommand(token: "/model", description: "Model", inputHint: nil, insertion: "/model", unavailableReason: "Desktop only")
    }

    /// Mirrors `Invocation::link()` for a file-backed skill: the payload is the
    /// hex-encoded canonical JSON, so restore can round-trip the path identity.
    private func skill(_ token: String, name: String, path: String) -> ComposerCommand {
        let json = "{\"kind\":\"skill\",\"name\":\"\(name)\",\"path\":\"\(path)\"}"
        let payload = json.utf8.map { String(format: "%02x", $0) }.joined()
        return ComposerCommand(token: token, description: "", inputHint: nil,
                               insertion: "[$\(name)](zeron-invoke:\(payload))",
                               unavailableReason: nil)
    }

    @MainActor
    func testSkillDisplayNameMatchesDesktopCompletion() {
        XCTAssertEqual(CommandSuggestions.displayName(for: skill("$imagegen", name: "imagegen", path: "/Users/dev/.agents/skills/imagegen/SKILL.md")), "Imagegen")
        XCTAssertEqual(CommandSuggestions.displayName(for: skill("$skill_creator", name: "skill_creator", path: "/skills/skill_creator/SKILL.md")), "Skill Creator")
        XCTAssertEqual(CommandSuggestions.displayName(for: skill("$plugin:review-agent", name: "plugin:review-agent", path: "/skills/review-agent/SKILL.md")), "Review Agent")
        XCTAssertEqual(CommandSuggestions.displayName(for: skill("$review2", name: "review2", path: "/skills/review2/SKILL.md")), "Review2")
        XCTAssertEqual(CommandSuggestions.displayName(for: skill("$2fa", name: "2fa", path: "/skills/2fa/SKILL.md")), "2fa")
        XCTAssertEqual(CommandSuggestions.displayName(for: modelCommand), "/model")
    }

    @MainActor
    func testSameNameSkillDisambiguationKeepsNameAndEncodedPath() {
        let first = skill("$review", name: "review", path: "/a/SKILL.md")
        let second = skill("$review:2", name: "review", path: "/b/SKILL.md")
        XCTAssertEqual(CommandSuggestions.displayName(for: first), "Review")
        XCTAssertEqual(CommandSuggestions.displayName(for: second), "Review · 2")
        XCTAssertNotEqual(first.insertion, second.insertion)

        let restored = restoreComposerDraft(text: second.insertion)
        XCTAssertEqual(restored.text, "$review", "the catalog suffix is presentation-only")
        XCTAssertEqual(restored.selections.first?.insertion, second.insertion, "the encoded /b path identity is preserved")
        XCTAssertEqual(restoreComposerDraft(text: first.insertion).selections.first?.insertion, first.insertion)
        XCTAssertEqual(second.token, "$review:2", "the canonical selection token is not rewritten")
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
    func testNamespacedActionsExecuteAcrossOtherAgents() async {
        for harness in ["claude-code", "copilot", "cursor", "devin", "grok", "hermes", "pi", "omp", "opencode", "antigravity", "dsh"] {
            let composer = ComposerBar()
            composer.commandHarness = { harness }
            composer.localCommandContext = { self.context }
            let invoked = expectation(description: "\(harness) local action")
            composer.onLocalCommand = { action in
                XCTAssertEqual(action, .model)
                invoked.fulfill()
            }
            composer.onSend = { _, _, _ in XCTFail("Local action must not send a prompt"); return .kept }
            composer.text = "/zeron:mo keep this"
            composer.textView.selectedRange = NSRange(location: 9, length: 0)
            composer.insertCommand(ComposerCommand(token: "/zeron:model", description: "Zerun: Model", inputHint: nil, insertion: "/zeron:model", unavailableReason: "Mobile action"))
            XCTAssertEqual(composer.encodedDraft, " keep this")
            await fulfillment(of: [invoked], timeout: 2)
            let submitted = expectation(description: "\(harness) direct action")
            composer.onLocalCommand = { action in
                XCTAssertEqual(action, .model)
                submitted.fulfill()
            }
            composer.text = "/zeron:model"
            func sendButton(_ view: UIView) -> UIButton? {
                if view.accessibilityIdentifier == "composer-send" { return view as? UIButton }
                return view.subviews.compactMap(sendButton).first
            }
            sendButton(composer)!.sendActions(for: .touchUpInside)
            await fulfillment(of: [submitted], timeout: 2)
            XCTAssertEqual(composer.text, "")
        }
    }

    @MainActor
    func testOtherAgentsNativeCommandsStillReachSend() {
        for (harness, token) in [("claude-code", "/model"), ("pi", "/name"), ("opencode", "/review"), ("dsh", "/compact")] {
            let composer = ComposerBar()
            composer.commandHarness = { harness }
            composer.localCommandContext = { self.context }
            composer.onLocalCommand = { _ in XCTFail("Must preserve native command routing") }
            var sent: String?
            composer.onSend = { body, _, _ in sent = body; return .sent }
            composer.text = "\(token) native argument"
            func sendButton(_ view: UIView) -> UIButton? {
                if view.accessibilityIdentifier == "composer-send" { return view as? UIButton }
                return view.subviews.compactMap(sendButton).first
            }
            sendButton(composer)!.sendActions(for: .touchUpInside)
            XCTAssertEqual(sent, "\(token) native argument")
        }
    }

    @MainActor
    func testLateMenuClickFromPreviousContextIsIgnored() {
        let suggestions = CommandSuggestions()
        suggestions.context = "old-host:codex"
        var picked = false
        suggestions.onPick = { _ in picked = true }
        var command = modelCommand
        command.unavailableReason = nil
        suggestions.show(ComposerCatalog(commands: [command], warning: nil), query: "")
        func commandList(_ view: UIView) -> UITableView? {
            if let table = view as? UITableView { return table }
            return view.subviews.compactMap(commandList).first
        }
        let table = commandList(suggestions)!
        suggestions.context = "new-host:claude-code"
        table.delegate?.tableView?(table, didSelectRowAt: IndexPath(row: 0, section: 0))
        XCTAssertFalse(picked)
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
