import UIKit
import XCTest
@testable import Zeron

final class ModelCatalogRecoveryTests: XCTestCase {
    func testBriefRouteLossChangesCatalogReachabilityWhileBannerStaysConnected() {
        func connection(online: Bool) -> SessionCatalogConnection {
            SessionCatalogConnection(device: "host", hostAvailable: true, network: .connected, pathOnline: online)
        }
        let offline = connection(online: false)
        XCTAssertFalse(offline.reachable)
        let recovered = connection(online: true)
        XCTAssertTrue(recovered.reachable)
        XCTAssertNotEqual(offline, recovered)
        XCTAssertFalse(SessionCatalogConnection(device: "host", hostAvailable: false, network: .connected, pathOnline: true).reachable)
    }

    @MainActor
    func testOpenPickerPageDiscardsAnOfflineResponseThatArrivesAfterReload() async throws {
        let entered = expectation(description: "offline request started")
        let replies = ModelReplies(entered)
        let page = ModelPickerPage(tab: ModelPickerTab(harness: "codex", label: "Codex") { await replies.load() },
                                   current: ("codex", nil), notice: nil)
        var loads = 0
        page.onLoaded = { loads += 1 }
        let offline = page.load()
        await fulfillment(of: [entered], timeout: 2)
        // The same page recovers, without closing/reopening the picker.
        await page.load().value
        let shown = try XCTUnwrap(page.list.dataSource)
        XCTAssertEqual(shown.collectionView(page.list, numberOfItemsInSection: 0), 2)
        await replies.finishOffline()
        await offline.value
        XCTAssertEqual(shown.collectionView(page.list, numberOfItemsInSection: 0), 2)
        XCTAssertEqual(loads, 1)
    }
}

private actor ModelReplies {
    private let entered: XCTestExpectation
    private var offline: CheckedContinuation<[ModelPickerRow], Never>?

    init(_ entered: XCTestExpectation) { self.entered = entered }

    func load() async -> [ModelPickerRow] {
        if offline != nil {
            return ["current", "new"].map { ModelPickerRow(id: $0, label: $0, detail: nil) }
        }
        return await withCheckedContinuation {
            offline = $0
            entered.fulfill()
        }
    }

    func finishOffline() {
        offline?.resume(returning: [])
        offline = nil
    }
}
