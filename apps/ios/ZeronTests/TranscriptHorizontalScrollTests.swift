import XCTest
import UIKit
@testable import Zeron

@MainActor
final class TranscriptHorizontalScrollTests: XCTestCase {
    private func model(key: UInt64 = 1, version: UInt64 = 1, width: Float = 280, contentWidth: Float = 900, detail: UInt64 = 10) -> RowModel {
        RowModel(display: RowDisplay(
            key: key, version: version, width: width, height: 80, text: "", runs: [], glyphs: [], boxes: [], links: [],
            scrollers: [Scroller(x: 0, y: 30, w: width, h: 40, contentWidth: contentWidth)],
            widgets: [Widget(id: 0, kind: .toolToggle(detail: detail, open: true), x: 0, y: 0, w: width, h: 30, payload: nil, scroller: nil)],
            fades: [], copyText: ""
        ), fonts: StyleFonts())
    }

    func testStreamingRefreshPreservesHorizontalPositionAndResizeClampsIt() throws {
        let row = RowView()
        row.configure(model(), kind: .tools)
        let scroll = try XCTUnwrap(row.subviews.compactMap { $0 as? UIScrollView }.first)
        scroll.contentOffset = CGPoint(x: 400, y: 0)

        row.configure(model(version: 2, contentWidth: 1200), kind: .tools)
        XCTAssertEqual(scroll.contentOffset.x, 400, accuracy: 0.5)

        row.configure(model(version: 3, width: 800, contentWidth: 900), kind: .tools)
        XCTAssertEqual(scroll.contentOffset.x, 100, accuracy: 0.5)

        row.configure(model(version: 4, width: 1000, contentWidth: 1000), kind: .tools)
        XCTAssertEqual(scroll.contentOffset.x, 0, accuracy: 0.5)
    }

    func testReusedRowAndDifferentToolDetailStartAtLeadingEdge() throws {
        let row = RowView()
        row.configure(model(), kind: .tools)
        let scroll = try XCTUnwrap(row.subviews.compactMap { $0 as? UIScrollView }.first)
        scroll.contentOffset = CGPoint(x: 250, y: 0)

        row.configure(model(key: 2), kind: .tools)
        XCTAssertEqual(scroll.contentOffset.x, 0, accuracy: 0.5)
        scroll.contentOffset = CGPoint(x: 250, y: 0)

        row.configure(model(key: 2, version: 2, detail: 20), kind: .tools)
        XCTAssertEqual(scroll.contentOffset.x, 0, accuracy: 0.5)
    }
}
