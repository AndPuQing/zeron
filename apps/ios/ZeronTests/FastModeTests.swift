import XCTest
@testable import Zeron

@MainActor
final class FastModeTests: XCTestCase {
    private func option(_ id: String, _ choices: [String], _ fallback: String) -> ModelOption {
        ModelOption(id: id, label: "Speed", choices: choices.map { ModelOptionChoice(id: $0, label: $0) }, defaultChoice: fallback)
    }

    func testCatalogEncodingsAndExplicitOff() throws {
        let cases: [(String, [String], String, String, String)] = [
            ("serviceTier", ["default", "fast"], "default", "fast", "default"),
            ("fastMode", ["off", "on"], "off", "on", "off"),
            ("fast_mode", ["off", "on"], "off", "on", "off"),
            ("fast", ["false", "true"], "true", "true", "false"),
            ("speed", ["normal", "fast"], "normal", "fast", "normal"),
        ]
        for (id, choices, fallback, on, off) in cases {
            let fast = try XCTUnwrap(FastModeOption(option(id, choices, fallback)))
            let initial = ["contextWindow": "1m"]
            XCTAssertEqual(fast.title(in: initial), "Fast: Default")
            let enabled = fast.applying(true, to: initial)
            XCTAssertEqual(enabled[id], on)
            XCTAssertEqual(fast.enabled(in: enabled), true)
            let disabled = fast.applying(false, to: enabled)
            XCTAssertEqual(disabled[id], off, "Off must not remove an override and inherit Fast again")
            XCTAssertEqual(disabled["contextWindow"], "1m")
            XCTAssertEqual(fast.enabled(in: disabled), false)
            XCTAssertEqual(fast.applying(nil, to: disabled), initial)
        }
    }

    func testLegacyValuesAndUnsupportedCatalogs() throws {
        let claude = try XCTUnwrap(FastModeOption(option("fastMode", ["off", "on"], "off")))
        XCTAssertEqual(claude.enabled(in: ["fastMode": "true"]), true)
        XCTAssertEqual(claude.enabled(in: ["fastMode": "false"]), false)
        let codex = try XCTUnwrap(FastModeOption(option("serviceTier", ["default", "fast"], "default")))
        XCTAssertEqual(codex.enabled(in: ["serviceTier": "priority"]), true)
        XCTAssertNil(FastModeOption.find(in: []))
        XCTAssertNil(FastModeOption(option("speed", ["fast"], "fast")))
        XCTAssertNil(FastModeOption(option("contextWindow", ["200k", "1m"], "200k")))
    }

    func testDraftRetainsFastAndReadsOlderSavedDrafts() throws {
        let old = Data(#"{"harness":"codex","worktree":false,"model":"gpt-test"}"#.utf8)
        var draft = try JSONDecoder().decode(NewSessionDraft.self, from: old)
        XCTAssertNil(draft.modelOptions)
        draft.modelOptions = ["serviceTier": "default", "contextWindow": "1m"]
        let restored = try JSONDecoder().decode(NewSessionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(restored, draft)
    }

    func testExplicitCodexOffSurvivesModelChangesAndDraftRestore() throws {
        // A Fast default must not win again after On -> Off -> another model.
        let fast = try XCTUnwrap(FastModeOption(option("serviceTier", ["default", "fast"], "fast")))
        let other = ["contextWindow": "1m"]
        let off = fast.applying(false, to: fast.applying(true, to: other))
        let changed = FastModeOption.afterModelChange(off, harness: "codex", previousOptionID: fast.id)
        XCTAssertEqual(changed, ["serviceTier": "default", "contextWindow": "1m"])
        // Switching onward from a model/catalog without speed choices also keeps the reset.
        XCTAssertEqual(FastModeOption.afterModelChange(changed, harness: "codex", previousOptionID: nil), changed)
        var draft = NewSessionDraft()
        draft.harness = "codex"
        draft.model = "another-model"
        draft.modelOptions = changed
        let restored = try JSONDecoder().decode(NewSessionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(restored.modelOptions, changed)
        XCTAssertEqual(fast.enabled(in: restored.modelOptions ?? [:]), false)
        XCTAssertEqual(fast.applying(nil, to: changed), other, "Use default must still explicitly remove the reset")
    }

    func testModelChangeStillPrunesFastOnAndDoesNotCreateAnOffOverride() {
        let other = ["contextWindow": "1m"]
        for value in ["fast", "priority"] {
            XCTAssertEqual(FastModeOption.afterModelChange(["serviceTier": value, "contextWindow": "1m"], harness: "codex", previousOptionID: "serviceTier"), other)
        }
        XCTAssertEqual(FastModeOption.afterModelChange(other, harness: "codex", previousOptionID: "serviceTier"), other)
        XCTAssertEqual(FastModeOption.afterModelChange([:], harness: "codex", previousOptionID: "serviceTier"), [:])
        // The exception belongs only to Codex; other providers retain their pruning behavior.
        XCTAssertEqual(FastModeOption.afterModelChange(["serviceTier": "default"], harness: "other", previousOptionID: "serviceTier"), [:])
        XCTAssertEqual(FastModeOption.afterModelChange(["fastMode": "off", "contextWindow": "1m"], harness: "claude-code", previousOptionID: "fastMode"), other)
    }
}
