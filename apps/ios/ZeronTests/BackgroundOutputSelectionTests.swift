import XCTest
@testable import Zeron

final class BackgroundOutputSelectionTests: XCTestCase {
    func testLateOutputCannotMoveFromAToB() throws {
        var selection = BackgroundOutputSelection()
        selection.toggle("a")
        let delayedA = try XCTUnwrap(selection.request)
        selection.toggle("b")
        XCTAssertFalse(selection.accepts(delayedA))
        let currentB = try XCTUnwrap(selection.request)
        XCTAssertEqual(currentB.taskId, "b")
        XCTAssertTrue(selection.accepts(currentB))
    }

    func testReturningToAStillRejectsTheOldRequest() throws {
        var selection = BackgroundOutputSelection()
        selection.toggle("a")
        let delayedA = try XCTUnwrap(selection.request)
        selection.toggle("b")
        selection.toggle("a")
        XCTAssertFalse(selection.accepts(delayedA))
        XCTAssertTrue(selection.accepts(try XCTUnwrap(selection.request)))
    }

    func testCollapseAndDismissInvalidateInFlightOutput() throws {
        var selection = BackgroundOutputSelection()
        selection.toggle("a")
        let beforeCollapse = try XCTUnwrap(selection.request)
        selection.toggle("a")
        XCTAssertNil(selection.request)
        XCTAssertFalse(selection.accepts(beforeCollapse))
        selection.toggle("a")
        let beforeDismiss = try XCTUnwrap(selection.request)
        selection.invalidate()
        XCTAssertFalse(selection.accepts(beforeDismiss))
        XCTAssertEqual(selection.id, "a", "reopening may retain the selection, not its old response")
    }

    func testRemovalInvalidatesOutputWithoutDisturbingASurvivingTask() throws {
        var selection = BackgroundOutputSelection()
        selection.toggle("a")
        let request = try XCTUnwrap(selection.request)
        selection.retain(available: ["a", "b"])
        XCTAssertTrue(selection.accepts(request))
        selection.retain(available: ["b"])
        XCTAssertNil(selection.request)
        XCTAssertFalse(selection.accepts(request))
        selection.toggle("a")
        XCTAssertFalse(selection.accepts(request), "reused task id must not revive a stale response")
    }
}
