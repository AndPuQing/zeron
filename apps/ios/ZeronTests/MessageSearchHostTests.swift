import XCTest
@testable import Zeron

final class MessageSearchHostTests: XCTestCase {
    private func host(_ id: String, online: Bool) -> DeviceView {
        DeviceView(id: id, name: "Computer \(id)", platform: "macos", online: online,
                   lastSeenMs: nil, version: nil, capabilities: [], isExecutionHost: true,
                   isSelf: false, sessionCount: 0)
    }

    func testInitialChoiceWaitsForDevicesAndPrefersOnline() {
        XCTAssertNil(retainedMessageSearchHost(nil, hosts: []))
        XCTAssertEqual(retainedMessageSearchHost(nil, hosts: [host("a", online: false), host("b", online: true)])?.id, "b")
        XCTAssertEqual(retainedMessageSearchHost(nil, hosts: [host("a", online: false)])?.id, "a")
    }

    func testPresenceChangesDoNotReplaceTheInitialChoice() {
        let selected = retainedMessageSearchHost(nil, hosts: [host("a", online: false), host("b", online: true)])
        let next = retainedMessageSearchHost(selected, hosts: [host("a", online: true), host("b", online: false)])
        XCTAssertEqual(next?.id, "b")
        XCTAssertEqual(next?.online, false)
    }

    func testExplicitSelectionSurvivesRosterGapsAndRecovers() {
        let selected = host("b", online: true)
        let remaining = [host("a", online: true)]
        let missing = retainedMessageSearchHost(selected, hosts: remaining)
        XCTAssertEqual(missing, selected)
        XCTAssertNil(remaining.first { $0.id == missing?.id }) // unavailable, not cached online
        XCTAssertEqual(retainedMessageSearchHost(missing, hosts: []), selected)
        var renamed = selected
        renamed.name = "Renamed"; renamed.online = false
        XCTAssertEqual(retainedMessageSearchHost(missing, hosts: remaining + [renamed]), renamed)
        XCTAssertEqual(retainedMessageSearchHost(renamed, hosts: [selected]), selected)
        XCTAssertEqual(retainedMessageSearchHost(host("a", online: true), hosts: remaining)?.id, "a")
    }
}
