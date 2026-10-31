import XCTest
import UIKit
@testable import Zeron

@MainActor
final class TranscriptImageReadsTests: XCTestCase {
    @MainActor private final class Gate {
        var continuation: CheckedContinuation<UIImage?, Never>?
        func read(_ started: XCTestExpectation) async -> UIImage? {
            await withCheckedContinuation { continuation in
                self.continuation = continuation
                started.fulfill()
            }
        }
        func finish() { continuation?.resume(returning: UIImage()); continuation = nil }
    }

    func testDiscardedQueuedImageNeverReadsOrDelaysTheVisibleImage() async {
        let queue = TranscriptImageReads(limit: 1)
        let gate = Gate()
        let started = expectation(description: "first read started")
        queue.request(key: "first", consumer: UUID(), operation: { await gate.read(started) }, completion: { _ in })
        await fulfillment(of: [started], timeout: 2)
        let gone = UUID()
        var obsoleteReads = 0
        var cancellations = 0
        queue.request(key: "gone", consumer: gone, operation: { obsoleteReads += 1; return nil }, completion: {
            if case .cancelled = $0 { cancellations += 1 }
        })
        queue.cancel(key: "gone", consumer: gone)
        let visible = expectation(description: "visible read completed")
        queue.request(key: "visible", consumer: UUID(), operation: { UIImage() }, completion: {
            if case let .loaded(image) = $0 { XCTAssertNotNil(image); visible.fulfill() }
        })
        gate.finish()
        await fulfillment(of: [visible], timeout: 2)
        XCTAssertEqual(obsoleteReads, 0)
        XCTAssertEqual(cancellations, 1)
    }

    func testOneConsumerLeavingDoesNotCancelAnotherConsumersSharedImage() async {
        let queue = TranscriptImageReads(limit: 1)
        let gate = Gate()
        let started = expectation(description: "shared read started")
        let first = UUID()
        var cancelled = false
        queue.request(key: "same", consumer: first, operation: { await gate.read(started) }, completion: {
            if case .cancelled = $0 { cancelled = true }
        })
        await fulfillment(of: [started], timeout: 2)
        let received = expectation(description: "remaining consumer received image")
        queue.request(key: "same", consumer: UUID(), operation: { XCTFail("duplicate read"); return nil }, completion: {
            if case let .loaded(image) = $0 { XCTAssertNotNil(image); received.fulfill() }
        })
        queue.cancel(key: "same", consumer: first)
        XCTAssertTrue(cancelled)
        gate.finish()
        await fulfillment(of: [received], timeout: 2)
    }

    func testCancelledActiveReadCannotRemoveItsReplacementOrExceedTheLimit() async {
        let queue = TranscriptImageReads(limit: 1)
        let old = Gate()
        let started = expectation(description: "old started")
        let consumer = UUID()
        var oldCallbacks = 0
        queue.request(key: "same", consumer: consumer, operation: { await old.read(started) }, completion: {
            if case .loaded = $0 { XCTFail("cancelled result delivered") }
            oldCallbacks += 1
        })
        await fulfillment(of: [started], timeout: 2)
        queue.cancel(key: "same", consumer: consumer)
        let replacement = Gate()
        let replacementStarted = expectation(description: "replacement started")
        let received = expectation(description: "replacement received")
        var starts = 0
        queue.request(key: "same", consumer: UUID(), operation: {
            starts += 1
            return await replacement.read(replacementStarted)
        }, completion: { if case .loaded = $0 { received.fulfill() } })
        await Task.yield()
        XCTAssertEqual(starts, 0, "the cancelled operation still occupies its slot until it exits")
        old.finish()
        await fulfillment(of: [replacementStarted], timeout: 2)
        let shared = expectation(description: "replacement remains shared")
        queue.request(key: "same", consumer: UUID(), operation: { XCTFail("replacement was removed"); return nil }, completion: {
            if case .loaded = $0 { shared.fulfill() }
        })
        replacement.finish()
        await fulfillment(of: [received, shared], timeout: 2)
        XCTAssertEqual(starts, 1)
        XCTAssertEqual(oldCallbacks, 1)
    }

    func testCancellationBeforeTaskStartsSkipsTheOperation() async {
        let queue = TranscriptImageReads(limit: 1)
        let consumer = UUID()
        var starts = 0
        queue.request(key: "gone", consumer: consumer, operation: { starts += 1; return nil }, completion: { _ in })
        queue.cancel(key: "gone", consumer: consumer)
        let done = expectation(description: "next read completed")
        queue.request(key: "next", consumer: UUID(), operation: { nil }, completion: { _ in done.fulfill() })
        await fulfillment(of: [done], timeout: 2)
        XCTAssertEqual(starts, 0)
    }

    func testImageViewStartsOnlyInAWindowAndReleasesOnRemovalAndRebinding() {
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 100, height: 100))
        let view = TranscriptImageView()
        var starts = 0
        var cancels = 0
        let bind: (TranscriptImageView) -> (() -> Void) = { _ in
            starts += 1
            return { cancels += 1 }
        }
        view.bindImageLoad(bind)
        XCTAssertEqual(starts, 0)
        window.addSubview(view)
        XCTAssertEqual(starts, 1)
        view.removeFromSuperview()
        XCTAssertEqual(cancels, 1)
        window.addSubview(view)
        XCTAssertEqual(starts, 2)
        view.bindImageLoad(bind)
        XCTAssertEqual(cancels, 2)
        XCTAssertEqual(starts, 3)
        // Pooled rows stay attached to the window but are no longer consumers.
        view.active = false
        XCTAssertEqual(cancels, 3)
        view.active = true
        XCTAssertEqual(starts, 4)
        view.removeFromSuperview()
        XCTAssertEqual(cancels, 4)
    }
}
