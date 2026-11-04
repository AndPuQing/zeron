import XCTest
import UIKit
@testable import Zeron

private final class DocumentReaderClient: CoreClient, @unchecked Sendable {
    let preview: DocumentPreview?
    init(_ preview: DocumentPreview?) {
        self.preview = preview
        super.init(noHandle: .init())
    }
    required init(unsafeFromHandle handle: UInt64) { fatalError() }
    override func readDocument(chatId: String, path: String, allowOutside: Bool) async throws -> DocumentRead {
        guard let preview else { throw CoreError.HostUnavailable(message: "The host is offline. Reconnect it and try again.") }
        return .ready(document: preview)
    }
    override func devices() -> [DeviceView] { [] }
}

@MainActor
final class DocumentReaderAppearanceTests: XCTestCase {
    private let text = """
    # A calmer place to read

    Open documents from your computer, right here on your phone.

    ## Keep your context

    - **Preview** for reading, **Source** for the original text.
    - Follow links to related documents.
    - Go back without losing your place.

    > Your files stay on their host. This view is read-only.

    ## A small example

    ```swift
    let document = "guide.md"
    print(document)
    ```

    [Continue reading](next.md)
    """

    func testReaderModesAndStatesCaptureNativeAppearance() async throws {
        let oldWindow = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).first { $0.isKeyWindow }
        let scene = try XCTUnwrap(oldWindow?.windowScene)
        let window = UIWindow(windowScene: scene)
        window.frame = scene.coordinateSpace.bounds
        defer { window.isHidden = true; oldWindow?.makeKeyAndVisible() }
        let path = "/workspace/docs/getting-started-with-remote-documents.md"
        let doc = DocumentPreview(path: path, text: text, markdown: true, truncated: false, deviceId: "host", cwd: "/workspace")
        let reader = DocumentPreviewViewController(client: DocumentReaderClient(doc), chatId: "chat", path: path)
        window.overrideUserInterfaceStyle = .light
        window.rootViewController = UINavigationController(rootViewController: reader)
        window.makeKeyAndVisible()
        let list = try XCTUnwrap(descendants(reader.view).compactMap { $0 as? TranscriptListView }.first)
        try await until { list.contentSize.height > 100 }
        XCTAssertEqual(list.contentOffset.y, -list.adjustedContentInset.top, accuracy: 1)
        let mode = try XCTUnwrap(descendants(reader.view).compactMap { $0 as? UISegmentedControl }.first)
        XCTAssertFalse(mode.isHidden)
        capture(window, "document-ios-preview-light")
        window.overrideUserInterfaceStyle = .dark
        mode.selectedSegmentIndex = 1
        mode.sendActions(for: .valueChanged)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(mode.selectedSegmentIndex, 1)
        capture(window, "document-ios-source-dark")

        let empty = DocumentPreview(path: "empty.md", text: "", markdown: true, truncated: false, deviceId: "host", cwd: "/workspace")
        for (name, preview, title) in [("empty", Optional(empty), "This document is empty"), ("error", nil, "Couldn't open document")] {
            window.overrideUserInterfaceStyle = .light
            let page = DocumentPreviewViewController(client: DocumentReaderClient(preview), chatId: "chat", path: "notes.md")
            window.rootViewController = UINavigationController(rootViewController: page)
            try await until { self.descendants(page.view).compactMap { $0 as? UILabel }.contains { $0.text == title } }
            window.layoutIfNeeded()
            capture(window, "document-ios-\(name)")
        }
    }

    private func descendants(_ view: UIView) -> [UIView] {
        [view] + view.subviews.flatMap { descendants($0) }
    }
    private func until(_ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(10)
        while !ready() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(ready())
    }
    private func capture(_ window: UIWindow, _ name: String) {
        window.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
            window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
        }
        let attachment = XCTAttachment(image: image)
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
