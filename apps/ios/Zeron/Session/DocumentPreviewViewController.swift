import UIKit

/// A read-only document fetched from the originating chat's execution host.
final class DocumentPreviewViewController: UIViewController {
    private let client: CoreClient
    private let chatId: String
    private let path: String
    private var document: DocumentPreview?
    private var source = false
    private var request: Task<Void, Never>?
    private lazy var relay = FrameRelay { [weak self] in
        guard let self else { return }
        self.list.apply(self.engine.frame())
    }
    private lazy var engine = TranscriptView(text: TextEngine.shared, listener: relay)
    private lazy var list = TranscriptListView(engine: engine, startsAtTop: true)
    private let notice = UILabel()
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
        view.backgroundColor = Palette.background
        navigationItem.largeTitleDisplayMode = .never
        if navigationController?.viewControllers.first === self {
            navigationItem.leftBarButtonItem = UIBarButtonItem(systemItem: .close, primaryAction: UIAction { [weak self] _ in self?.dismiss(animated: true) })
        }
        notice.text = "Read-only · \(path)"
        notice.font = .preferredFont(forTextStyle: .caption1)
        notice.textColor = Palette.secondary
        notice.numberOfLines = 3
        notice.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(notice)
        list.translatesAutoresizingMaskIntoConstraints = false
        list.onOpenDocument = { [weak self] url in
            guard let self, let target = documentLinkPath(url: url, basePath: self.path) else { return false }
            // A link to this document (`guide.md#usage`) stays on it.
            if target != self.path {
                self.navigationController?.pushViewController(DocumentPreviewViewController(client: self.client, chatId: self.chatId, path: target), animated: true)
            }
            return true
        }
        view.addSubview(list)
        setContentScrollView(list, for: .top)
        status.font = .preferredFont(forTextStyle: .body)
        status.textColor = Palette.text
        status.numberOfLines = 0
        status.textAlignment = .center
        retry.setTitle("Retry", for: .normal)
        retry.addAction(UIAction { [weak self] _ in self?.loadDocument() }, for: .touchUpInside)
        state.axis = .vertical
        state.alignment = .center
        state.spacing = 16
        [spinner, status, retry].forEach(state.addArrangedSubview)
        state.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(state)
        NSLayoutConstraint.activate([
            notice.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 8),
            notice.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            notice.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            list.topAnchor.constraint(equalTo: notice.bottomAnchor, constant: 8),
            list.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            list.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            list.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            state.centerYAnchor.constraint(equalTo: view.safeAreaLayoutGuide.centerYAnchor),
            state.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 24),
            state.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -24),
        ])
        loadDocument()
    }

    private func loadDocument() {
        request?.cancel()
        state.isHidden = false
        list.isHidden = true
        status.text = "Loading document…"
        retry.isHidden = true
        spinner.startAnimating()
        let client = self.client, chatId = self.chatId, path = self.path
        request = Task { @MainActor [weak self] in
            do {
                let doc = try await client.readDocument(chatId: chatId, path: path)
                guard !Task.isCancelled, let self else { return }
                self.document = doc
                self.spinner.stopAnimating()
                self.state.isHidden = !doc.text.isEmpty
                self.status.text = "This document is empty."
                self.list.isHidden = doc.text.isEmpty
                self.notice.text = "Read-only · \(path)" + (doc.truncated ? "\nPreview truncated to the first 2 MiB." : "")
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
                self.spinner.stopAnimating()
                self.status.text = CloudStore.message(error)
                self.retry.isHidden = false
            }
        }
    }

    private func renderDocument() {
        guard let document else { return }
        var items = [UIBarButtonItem(title: "Copy", primaryAction: UIAction { _ in UIPasteboard.general.string = document.text })]
        if document.markdown {
            items.append(UIBarButtonItem(title: source ? "Preview" : "Source", primaryAction: UIAction { [weak self] _ in
                guard let self else { return }
                self.source.toggle()
                self.renderDocument()
            }))
        }
        navigationItem.rightBarButtonItems = items
        list.releaseFollow()
        list.setContentOffset(CGPoint(x: 0, y: -list.adjustedContentInset.top), animated: false)
        engine.setDocument(chatId: chatId, document: document, markdown: document.markdown && !source)
    }

    deinit {
        request?.cancel()
        // Releasing TranscriptView shuts down its worker in Rust. Accessing
        // the lazy, main-actor engine from nonisolated deinit is invalid.
    }
}
