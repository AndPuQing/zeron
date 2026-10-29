import XCTest
@testable import Zeron

final class MessageSearchLifecycleTests: XCTestCase {
    func testReturningFromAHitRestartsTheUnchangedQuery() throws {
        let lifecycle = MessageSearchLifecycle()
        var starts = 0
        var token: UInt64?
        let restart = { starts += 1; token = lifecycle.beginQuery() }
        lifecycle.appear(restart: restart)
        let first = try XCTUnwrap(token)
        XCTAssertTrue(lifecycle.accepts(first))
        // An indexing response has been shown; navigating into a hit hides
        // the search page while another response remains in flight.
        lifecycle.disappear()
        XCTAssertFalse(lifecycle.accepts(first))
        lifecycle.appear(restart: restart)
        XCTAssertEqual(starts, 2, "returning must not require another query edit")
        XCTAssertFalse(lifecycle.accepts(first), "late old results cannot overwrite resumed search")
        XCTAssertTrue(lifecycle.accepts(try XCTUnwrap(token)))
    }

    func testQueryOrHostChangeInvalidatesEarlierResponses() throws {
        let lifecycle = MessageSearchLifecycle()
        lifecycle.appear(restart: {})
        let first = try XCTUnwrap(lifecycle.beginQuery())
        let replacement = try XCTUnwrap(lifecycle.beginQuery())
        XCTAssertFalse(lifecycle.accepts(first))
        XCTAssertTrue(lifecycle.accepts(replacement))
        // Empty queries also invalidate a pending result, without starting RPC.
        _ = lifecycle.beginQuery()
        XCTAssertFalse(lifecycle.accepts(replacement))
    }

    func testHiddenSearchUpdatesDoNotStartWork() throws {
        let lifecycle = MessageSearchLifecycle()
        XCTAssertNil(lifecycle.beginQuery())
        lifecycle.appear(restart: {})
        let first = try XCTUnwrap(lifecycle.beginQuery())
        lifecycle.disappear()
        XCTAssertNil(lifecycle.beginQuery())
        XCTAssertFalse(lifecycle.accepts(first))
        var resumed = false
        lifecycle.appear { resumed = lifecycle.beginQuery() != nil }
        XCTAssertTrue(resumed)
    }
}
