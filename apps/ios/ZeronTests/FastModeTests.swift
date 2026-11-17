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
}
