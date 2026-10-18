import UIKit

/// Remote history selection; the Rust controller owns discovery and batch lifetimes.
final class SessionImportViewController: UIViewController, UITableViewDataSource, UITableViewDelegate, UISearchBarDelegate, UIAdaptivePresentationControllerDelegate {
    private let app: AppModel
    private var importer: CoreSessionImport?
    private var state: SessionImportSnapshot?
    private var observation: AnyObject?
    private lazy var bridge = ImportListenerBridge(owner: self)
    private let hostButton = UIButton(type: .system)
    private let agents = UISegmentedControl(items: ["All agents", "Codex", "Claude Code"])
    private let search = UISearchBar()
    private let table = UITableView(frame: .zero, style: .insetGrouped)
    private let status = UILabel()
    private let primary = UIButton(type: .system)
    private let retry = UIButton(type: .system)
    private let progress = UIProgressView(progressViewStyle: .default)
    private let spinner = UIActivityIndicatorView(style: .medium)
    private var notices: [String] = []

    init(app: AppModel) {
        self.app = app
        super.init(nibName: nil, bundle: nil)
        title = "Import sessions"
    }
    required init?(coder: NSCoder) { fatalError() }
    deinit { importer?.dismiss() }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = Palette.background
        navigationItem.leftBarButtonItem = UIBarButtonItem(systemItem: .close, primaryAction: UIAction { [weak self] _ in self?.requestClose() })
        navigationItem.rightBarButtonItem = UIBarButtonItem(systemItem: .refresh, primaryAction: UIAction { [weak self] _ in self?.importer?.refresh() })
        hostButton.showsMenuAsPrimaryAction = true
        hostButton.contentHorizontalAlignment = .leading
        hostButton.configuration = .tinted()
        agents.selectedSegmentIndex = 0
        agents.addTarget(self, action: #selector(agentChanged), for: .valueChanged)
        search.placeholder = "Search sessions or folders"
        search.searchBarStyle = .minimal
        search.delegate = self
        let intro = UILabel()
        intro.text = "Import Codex and Claude Code history from your computer. Imports are saved in Archived; originals stay on the source device."
        intro.font = .preferredFont(forTextStyle: .subheadline)
        intro.textColor = Palette.secondary
        intro.numberOfLines = 0
        let header = UIStackView(arrangedSubviews: [intro, hostButton, agents, search])
        header.axis = .vertical
        header.spacing = 12
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.keyboardDismissMode = .onDrag
        table.estimatedRowHeight = 90
        table.rowHeight = UITableView.automaticDimension
        table.backgroundView = spinner
        status.font = .preferredFont(forTextStyle: .footnote)
        status.textColor = Palette.secondary
        status.numberOfLines = 0
        var buttonStyle = UIButton.Configuration.filled()
        buttonStyle.baseBackgroundColor = Palette.text
        buttonStyle.baseForegroundColor = Palette.background
        buttonStyle.cornerStyle = .capsule
        buttonStyle.contentInsets = NSDirectionalEdgeInsets(top: 14, leading: 20, bottom: 14, trailing: 20)
        primary.configuration = buttonStyle
        primary.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            if self.state?.running == true { self.importer?.cancel() }
            else { self.importer?.start(retry: false) }
        }, for: .touchUpInside)
        retry.configuration = .tinted()
        retry.setTitle("Retry remaining", for: .normal)
        retry.addAction(UIAction { [weak self] _ in self?.importer?.start(retry: true) }, for: .touchUpInside)
        let footer = UIStackView(arrangedSubviews: [progress, status, retry, primary])
        footer.axis = .vertical
        footer.spacing = 10
        for child in [header, table, footer] { child.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(child) }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            header.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            header.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            table.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 4),
            table.leadingAnchor.constraint(equalTo: view.leadingAnchor), table.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            table.bottomAnchor.constraint(equalTo: footer.topAnchor, constant: -8),
            footer.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            footer.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            footer.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor, constant: -12),
        ])
        importer = app.client?.sessionImport(listener: bridge)
        if let snapshot = importer?.snapshot() { receive(snapshot) }
        observation = app.observe { [weak self] in self?.importer?.updateHosts() }
    }
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        navigationController?.presentationController?.delegate = self
    }
    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        if isBeingDismissed || navigationController?.isBeingDismissed == true { importer?.dismiss(); observation = nil }
    }
    func presentationControllerShouldDismiss(_ presentationController: UIPresentationController) -> Bool { state?.running != true }
    func presentationControllerDidAttemptToDismiss(_ presentationController: UIPresentationController) { requestClose() }
    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) { importer?.dismiss(); observation = nil }
    private func requestClose() {
        guard state?.running == true else { importer?.dismiss(); dismiss(animated: true); return }
        let alert = UIAlertController(title: "Stop importing?", message: "Sessions already imported will stay in your workspace.", preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "Keep importing", style: .cancel))
        alert.addAction(UIAlertAction(title: "Stop and close", style: .destructive) { [weak self] _ in self?.importer?.dismiss(); self?.dismiss(animated: true) })
        present(alert, animated: true)
    }
    @objc private func agentChanged() { importer?.setProvider(provider: [nil, "codex", "claude-code"][agents.selectedSegmentIndex]) }
    func searchBar(_ searchBar: UISearchBar, textDidChange searchText: String) { importer?.setQuery(query: searchText) }

    fileprivate func receive(_ snapshot: SessionImportSnapshot) {
        guard isViewLoaded else { return }
        let deviceChanged = state?.deviceId != snapshot.deviceId
        state = snapshot
        let s = snapshot
        if deviceChanged { search.text = s.query }
        notices = [s.error, s.discoveryError].compactMap { $0 } + s.sourceErrors
        if s.deviceId == nil {
            notices.append(s.hosts.isEmpty ? "Connect a desktop host to discover sessions." : "Keep a compatible computer online. Update Zerun on the host if needed.")
        } else if s.hosts.first(where: { $0.id == s.deviceId })?.online == false {
            notices.append("This computer is offline. Reconnect it to import.")
        }
        hostButton.setTitle(s.hosts.first(where: { $0.id == s.deviceId })?.name ?? "Choose a computer", for: .normal)
        hostButton.menu = UIMenu(children: s.hosts.map { host in
            UIAction(title: host.name + (!host.supported ? " · Update required" : !host.online ? " · Offline" : ""),
                     attributes: host.online && host.supported ? [] : .disabled,
                     state: host.id == s.deviceId ? .on : .off) { [weak self] _ in self?.importer?.selectDevice(deviceId: host.id) }
        })
        hostButton.isEnabled = !s.running
        navigationItem.rightBarButtonItem?.isEnabled = !s.running && !s.loading
        agents.selectedSegmentIndex = s.provider == "codex" ? 1 : s.provider == "claude-code" ? 2 : 0
        agents.isHidden = s.deviceId == nil
        search.isHidden = s.deviceId == nil
        if s.loading { spinner.startAnimating() } else { spinner.stopAnimating() }
        let done = s.results.filter { $0.phase != .pending && $0.phase != .importing }.count
        let imported = s.results.filter { $0.phase == .imported || $0.phase == .alreadyManaged }.count
        progress.isHidden = !s.running
        progress.progress = s.results.isEmpty ? 0 : Float(done) / Float(s.results.count)
        status.text = s.running ? "\(done) of \(s.results.count) finished" : s.results.isEmpty
            ? "\(s.selectedCount) selected · up to \(s.selectionLimit) per import"
            : "\(imported) of \(s.results.count) imported · \(s.selectedCount) selected"
        primary.setTitle(s.running ? (s.cancelling ? "Stopping…" : "Stop import") : (s.selectedCount > 0 ? "Import \(s.selectedCount) selected" : "Select sessions to import"), for: .normal)
        primary.isEnabled = s.running ? !s.cancelling : s.selectedCount > 0 && !s.loading
        retry.isHidden = s.running || !s.results.contains(where: { $0.retryable })
        table.reloadData()
    }
    func numberOfSections(in tableView: UITableView) -> Int { 4 }
    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        guard let s = state else { return 0 }
        switch section {
        case 0: return notices.count + (s.discoveryError != nil && !s.running ? 1 : 0)
        case 1: return s.results.count
        case 2: return s.rows.isEmpty && !s.loading && s.deviceId != nil ? 1 : s.rows.count
        default: return s.deviceId == nil ? 0 : (s.hasMore ? 2 : 1)
        }
    }
    func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        if section == 1 && state?.results.isEmpty == false { return "Import results" }
        return section == 2 && state?.deviceId != nil ? "Not imported" : nil
    }
    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "import") ?? UITableViewCell(style: .subtitle, reuseIdentifier: "import")
        guard let s = state else { return cell }
        var content = cell.defaultContentConfiguration()
        content.textProperties.numberOfLines = 2
        content.secondaryTextProperties.numberOfLines = 0
        cell.accessoryType = .none
        cell.selectionStyle = .none
        cell.accessibilityTraits = .staticText
        switch indexPath.section {
        case 0:
            if indexPath.row < notices.count { content.text = notices[indexPath.row]; content.textProperties.color = Palette.secondary }
            else { content.text = "Retry discovery"; content.textProperties.color = Palette.accent; cell.accessibilityTraits = .button }
        case 1:
            let result = s.results[indexPath.row]
            content.text = result.title
            content.secondaryText = result.reason ?? phaseLabel(result.phase)
            content.secondaryTextProperties.color = result.phase == .failed ? .systemRed : Palette.secondary
        case 2:
            if s.rows.isEmpty {
                content.text = !s.query.isEmpty || s.provider != nil ? "No matching unimported sessions." : s.hasMore ? "No unimported sessions on this page. Load more to keep looking." : s.discoveryError != nil ? "Discovery could not finish. Try again." : "No unimported sessions found. Imported sessions are available in Archived."
                content.textProperties.color = Palette.secondary
            } else {
                let row = s.rows[indexPath.row]
                content.text = row.title.isEmpty ? "Untitled session" : row.title
                content.secondaryText = [(row.provider == "codex" ? "Codex" : "Claude Code"), row.cwd, row.disabledReason].compactMap { $0 }.joined(separator: "\n")
                content.textProperties.color = row.disabledReason == nil ? Palette.text : Palette.secondary
                cell.accessoryType = row.selected ? .checkmark : .none
                cell.selectionStyle = row.disabledReason == nil && !s.running && !s.loading ? .default : .none
                cell.accessibilityTraits = row.disabledReason == nil ? [.button] : [.notEnabled]
                if row.selected { cell.accessibilityTraits.insert(.selected) }
            }
        default:
            content.text = indexPath.row == 0 ? "Select visible" : "Load more sessions"
            content.textProperties.color = s.running || s.loading ? Palette.secondary : Palette.accent
            cell.accessibilityTraits = .button
        }
        var background = UIBackgroundConfiguration.listGroupedCell()
        background.backgroundColor = Palette.elevated
        cell.backgroundConfiguration = background
        cell.contentConfiguration = content
        return cell
    }
    func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        guard let s = state, !s.running, !s.loading else { return }
        switch indexPath.section {
        case 0:
            if indexPath.row == notices.count { if s.hasMore { importer?.loadMore() } else { importer?.refresh() } }
        case 2:
            if !s.rows.isEmpty { importer?.toggle(sourceRef: s.rows[indexPath.row].sourceRef) }
        case 3:
            if indexPath.row == 0 { importer?.selectVisible() } else { importer?.loadMore() }
        default: break
        }
    }
    private func phaseLabel(_ phase: SessionImportPhase) -> String {
        switch phase {
        case .pending: return "Waiting"
        case .importing: return "Importing…"
        case .imported: return "Imported · Archived"
        case .alreadyManaged: return "Already in Sessions"
        case .failed: return "Import failed"
        case .cancelled: return "Cancelled"
        }
    }
}

private final class ImportListenerBridge: SessionImportListener, @unchecked Sendable {
    weak var owner: SessionImportViewController?
    init(owner: SessionImportViewController) { self.owner = owner }
    func onImportChanged(snapshot: SessionImportSnapshot) {
        DispatchQueue.main.async { [weak self] in self?.owner?.receive(snapshot) }
    }
}
