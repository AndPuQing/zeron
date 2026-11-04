import UIKit

/// A read-only document fetched from the originating chat's execution host.
final class DocumentPreviewViewController: UIViewController {
    private let client: CoreClient
    private let chatId: String
    private let path: String
    private var document: DocumentPreview?
    private var source = false
    private var request: Task<Void, Never>?
    /// Canonical path the reader agreed to open; Retry keeps the same target.
    private var approvedPath: String?
    /// A confirmation that arrived before the view was on screen.
    private var pendingConfirmation: UIAlertController?
    private lazy var relay: FrameRelay = FrameRelay { [weak self] in
        guard let self else { return }
        self.list.apply(self.engine.frame())
    }
    private lazy var engine: TranscriptView = TranscriptView(text: TextEngine.shared, listener: relay)
    private lazy var list: TranscriptListView = TranscriptListView(engine: engine, startsAtTop: true)
    private let header = UIStackView()
    private let modeContainer = UIView()
    private let mode = UISegmentedControl(items: ["Preview", "Source"])
    private let notice = UILabel()
    private let statusTitle = UILabel()
    private let statusIcon = UIImageView()
    private let statusScroll = UIScrollView()
    private var copyItem: UIBarButtonItem?
    private let status = UILabel()
    private let spinner = UIActivityIndicatorView(style: .medium)
    private let retry = UIButton(type: .system)
    private let state = UIStackView()

    init(client: CoreClient, chatId: String, path: String) {
        self.client = client
        self.chatId = chatId
        self.path = path
        super.init(nibName: nil, bundle: nil)
        hidesBottomBarWhenPushed = true
        title = (path as NSString).lastPathComponent
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = Palette.elevated
        navigationItem.largeTitleDisplayMode = .never
        navigationItem.backButtonDisplayMode = .minimal
        if navigationController?.viewControllers.first === self {
            navigationItem.leftBarButtonItem = UIBarButtonItem(systemItem: .close, primaryAction: UIAction { [weak self] _ in self?.dismiss(animated: true) })
        }
        let copy = UIBarButtonItem(image: UIImage(systemName: "doc.on.doc"), primaryAction: UIAction { [weak self] _ in
            guard let self, let document = self.document else { return }
            UIPasteboard.general.string = document.text
            Toast.show("Copied", in: self.view.window)
        })
        copy.accessibilityLabel = "Copy document"
        copy.isEnabled = false
        copyItem = copy
        let more = UIBarButtonItem(image: UIImage(systemName: "ellipsis"), menu: UIMenu(children: [
            UIAction(title: "File details", image: UIImage(systemName: "info.circle")) { [weak self] _ in self?.showDetails() },
            UIAction(title: "Copy path", image: UIImage(systemName: "doc.on.doc")) { [weak self] _ in self?.copyPath() },
        ]))
        more.accessibilityLabel = "Document options"
        navigationItem.rightBarButtonItems = [more, copy]

        header.axis = .vertical
        header.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(header)
        mode.selectedSegmentIndex = 0
        mode.accessibilityIdentifier = "document-mode"
        mode.translatesAutoresizingMaskIntoConstraints = false
        mode.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            self.source = self.mode.selectedSegmentIndex == 1
            self.renderDocument()
        }, for: .valueChanged)
        modeContainer.addSubview(mode)
        modeContainer.isHidden = true
        header.addArrangedSubview(modeContainer)
        notice.text = "Showing the first 2 MiB"
        notice.font = .preferredFont(forTextStyle: .caption1)
        notice.adjustsFontForContentSizeCategory = true
        notice.textColor = Palette.secondary
        notice.textAlignment = .center
        notice.numberOfLines = 0
        notice.isHidden = true
        header.addArrangedSubview(notice)
        let divider = UIView()
        divider.backgroundColor = Palette.hairline
        divider.heightAnchor.constraint(equalToConstant: 0.5).isActive = true
        header.addArrangedSubview(divider)
        header.spacing = 8
        list.translatesAutoresizingMaskIntoConstraints = false
        list.accessibilityIdentifier = "document-reader"
        list.contentInset.top = 12
        list.contentInset.bottom = 24
        list.onOpenDocument = { [weak self] url in
            guard let self, let target = documentLinkPath(url: url, basePath: self.path) else { return false }
            if target != self.path {
                self.navigationController?.pushViewController(DocumentPreviewViewController(client: self.client, chatId: self.chatId, path: target), animated: true)
            }
            return true
        }
        view.addSubview(list)
        setContentScrollView(list, for: .top)

        statusTitle.font = .preferredFont(forTextStyle: .headline)
        statusTitle.textColor = Palette.text
        status.font = .preferredFont(forTextStyle: .subheadline)
        status.textColor = Palette.secondary
        for label in [statusTitle, status] {
            label.numberOfLines = 0
            label.textAlignment = .center
            label.adjustsFontForContentSizeCategory = true
        }
        statusIcon.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 36, weight: .light)
        statusIcon.tintColor = Palette.secondary
        statusIcon.contentMode = .scaleAspectFit
        var retryStyle = UIButton.Configuration.tinted()
        retryStyle.title = "Try again"
        retryStyle.cornerStyle = .capsule
        retryStyle.baseForegroundColor = Palette.accent
        retryStyle.contentInsets = NSDirectionalEdgeInsets(top: 10, leading: 20, bottom: 10, trailing: 20)
        retry.configuration = retryStyle
        retry.addAction(UIAction { [weak self] _ in self?.loadDocument() }, for: .touchUpInside)
        state.axis = .vertical
        state.alignment = .center
        state.spacing = 12
        [spinner, statusIcon, statusTitle, status, retry].forEach(state.addArrangedSubview)
        state.translatesAutoresizingMaskIntoConstraints = false
        statusScroll.translatesAutoresizingMaskIntoConstraints = false
        statusScroll.accessibilityIdentifier = "document-status"
        statusScroll.addSubview(state)
        view.addSubview(statusScroll)
        let readingWidth = list.widthAnchor.constraint(equalTo: view.safeAreaLayoutGuide.widthAnchor)
        readingWidth.priority = .defaultHigh
        let controlWidth = mode.widthAnchor.constraint(equalTo: modeContainer.widthAnchor, constant: -40)
        controlWidth.priority = .defaultHigh
        let stateWidth = state.widthAnchor.constraint(equalTo: statusScroll.frameLayoutGuide.widthAnchor, constant: -64)
        stateWidth.priority = .defaultHigh
        let centered = state.centerYAnchor.constraint(equalTo: statusScroll.frameLayoutGuide.centerYAnchor)
        centered.priority = .defaultLow
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            header.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
            header.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
            mode.topAnchor.constraint(equalTo: modeContainer.topAnchor, constant: 8),
            mode.bottomAnchor.constraint(equalTo: modeContainer.bottomAnchor),
            mode.centerXAnchor.constraint(equalTo: modeContainer.centerXAnchor),
            controlWidth, mode.widthAnchor.constraint(lessThanOrEqualToConstant: 360),
            list.topAnchor.constraint(equalTo: header.bottomAnchor),
            list.centerXAnchor.constraint(equalTo: view.safeAreaLayoutGuide.centerXAnchor),
            readingWidth, list.widthAnchor.constraint(lessThanOrEqualToConstant: 760),
            list.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            statusScroll.topAnchor.constraint(equalTo: header.bottomAnchor),
            statusScroll.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor),
            statusScroll.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
            statusScroll.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
            stateWidth, state.widthAnchor.constraint(lessThanOrEqualToConstant: 420),
            status.widthAnchor.constraint(equalTo: state.widthAnchor),
            statusTitle.widthAnchor.constraint(equalTo: state.widthAnchor),
            state.centerXAnchor.constraint(equalTo: statusScroll.frameLayoutGuide.centerXAnchor),
            centered,
            state.topAnchor.constraint(greaterThanOrEqualTo: statusScroll.contentLayoutGuide.topAnchor, constant: 32),
            state.bottomAnchor.constraint(equalTo: statusScroll.contentLayoutGuide.bottomAnchor, constant: -32),
            statusScroll.contentLayoutGuide.widthAnchor.constraint(equalTo: statusScroll.frameLayoutGuide.widthAnchor),
        ])
        loadDocument()
    }

    private func loadDocument() {
        request?.cancel()
        showStatus(title: "Opening document", detail: "Reading from the conversation's host…", loading: true)
        list.isHidden = true
        let client = self.client, chatId = self.chatId
        let path = approvedPath ?? self.path, allowOutside = approvedPath != nil
        request = Task { @MainActor [weak self] in
            do {
                let read = try await client.readDocument(chatId: chatId, path: path, allowOutside: allowOutside)
                guard !Task.isCancelled, let self else { return }
                var doc: DocumentPreview
                switch read {
                case let .ready(document):
                    doc = document
                case let .needsConfirmation(_, deviceName, outsidePath):
                    self.confirmOutside(device: deviceName, path: outsidePath)
                    return
                }
                // Relative links keep the document's original conversation context.
                doc.path = self.path
                self.document = doc
                self.showStatus(title: "This document is empty", detail: "There is no content to display.", icon: "doc.text")
                self.statusScroll.isHidden = !doc.text.isEmpty
                self.list.isHidden = doc.text.isEmpty
                self.notice.isHidden = !doc.truncated
                self.modeContainer.isHidden = !doc.markdown
                self.copyItem?.isEnabled = true
                self.list.imageLoader = { [weak self] ref, image in
                    guard let self else { return }
                    TranscriptImages.load(client: self.client, device: doc.deviceId, ref: ref, into: image)
                }
                self.list.imageFetcher = { [weak self] ref in
                    guard let self else { return nil }
                    return await TranscriptImages.image(client: self.client, device: doc.deviceId, ref: ref)
                }
                self.renderDocument()
            } catch {
                guard !Task.isCancelled, let self else { return }
                self.showStatus(title: "Couldn't open document", detail: CloudStore.message(error), icon: "doc.text.magnifyingglass", canRetry: true)
            }
        }
    }

    /// The preview opens only after the reader agrees. Chunk-capable hosts
    /// also defer reading; older hosts already returned the withheld contents.
    private func confirmOutside(device: String, path: String) {
        showStatus(title: "Waiting for permission", detail: "Confirm to read this file.", icon: "lock.doc")
        let alert = UIAlertController(
            title: "Open this file?",
            message: "This file is outside the chat's folder.\n\n\(device): \(path)",
            preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel) { [weak self] _ in self?.leave() })
        alert.addAction(UIAlertAction(title: "Open anyway", style: .default) { [weak self] _ in
            guard let self else { return }
            self.approvedPath = path
            self.loadDocument()
        })
        if view.window == nil {
            pendingConfirmation = alert
        } else {
            present(alert, animated: true)
        }
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        if let alert = pendingConfirmation {
            pendingConfirmation = nil
            present(alert, animated: true)
        }
    }

    private func leave() {
        if navigationController?.viewControllers.first === self || navigationController == nil {
            dismiss(animated: true)
        } else {
            navigationController?.popViewController(animated: true)
        }
    }

    private func renderDocument() {
        guard let document else { return }
        mode.selectedSegmentIndex = source ? 1 : 0
        list.releaseFollow()
        list.setContentOffset(CGPoint(x: 0, y: -list.adjustedContentInset.top), animated: false)
        engine.setDocument(chatId: chatId, document: document, markdown: document.markdown && !source)
    }

    private func showStatus(title: String, detail: String, icon: String? = nil, loading: Bool = false, canRetry: Bool = false) {
        statusScroll.isHidden = false
        statusTitle.text = title
        status.text = detail
        statusIcon.image = icon.flatMap { UIImage(systemName: $0) }
        statusIcon.isHidden = icon == nil
        spinner.isHidden = !loading
        if loading { spinner.startAnimating() } else { spinner.stopAnimating() }
        retry.isHidden = !canRetry
    }

    private func copyPath() {
        UIPasteboard.general.string = approvedPath ?? path
        Toast.show("Path copied", in: view.window)
    }

    private func showDetails() {
        let device = document.flatMap { doc in client.devices().first { $0.id == doc.deviceId }?.name }
        let message = "Read-only" + (device.map { " · \($0)" } ?? "") + "\n\n" + (approvedPath ?? path)
        let alert = UIAlertController(title: "File details", message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "Copy path", style: .default) { [weak self] _ in self?.copyPath() })
        alert.addAction(UIAlertAction(title: "Done", style: .cancel))
        present(alert, animated: true)
    }

    deinit {
        request?.cancel()
        // Releasing TranscriptView shuts down its worker in Rust. Accessing
        // the lazy, main-actor engine from nonisolated deinit is invalid.
    }
}
