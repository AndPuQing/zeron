import UIKit

/// Remote history selection; the Rust controller owns discovery and batch lifetimes.
final class SessionImportViewController: UIViewController, UITableViewDataSource, UITableViewDelegate, UISearchBarDelegate, UIAdaptivePresentationControllerDelegate {
    private let app: AppModel
    private var importer: CoreSessionImport?
    private var state: SessionImportSnapshot?
    private var observation: AnyObject?
    private lazy var bridge = ImportListenerBridge(owner: self)
    private let hostButton = UIButton(type: .system)
    private let agents = UIButton(type: .system)
    private let search = UISearchBar()
    private let table = UITableView(frame: .zero, style: .insetGrouped)
    private let status = UILabel()
    private let primary = UIButton(type: .system)
    private let retry = UIButton(type: .system)
    private let progress = UIProgressView(progressViewStyle: .default)
    private let spinner = UIActivityIndicatorView(style: .medium)
    private let header = UIStackView()
    private let searchGlass = Glass.surface()
    private let footerGlass = Glass.surface(radius: Glass.Metrics.cardRadius)
    private var didSetInsets = false
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
        agents.showsMenuAsPrimaryAction = true
        search.placeholder = "Search sessions"
        search.searchBarStyle = .minimal
        search.searchTextField.backgroundColor = .clear
        search.delegate = self
        let pickers = UIStackView(arrangedSubviews: [hostButton, agents])
        pickers.distribution = .fillEqually
        pickers.spacing = 10
        search.translatesAutoresizingMaskIntoConstraints = false
        searchGlass.contentView.addSubview(search)
        let searchHeight = searchGlass.heightAnchor.constraint(equalToConstant: 52)
        searchHeight.priority = .defaultHigh
        NSLayoutConstraint.activate([
            hostButton.heightAnchor.constraint(equalToConstant: Glass.Metrics.control),
            agents.heightAnchor.constraint(equalToConstant: Glass.Metrics.control),
            searchHeight,
            search.leadingAnchor.constraint(equalTo: searchGlass.contentView.leadingAnchor, constant: 4),
            search.trailingAnchor.constraint(equalTo: searchGlass.contentView.trailingAnchor, constant: -4),
            search.centerYAnchor.constraint(equalTo: searchGlass.contentView.centerYAnchor),
        ])
        header.addArrangedSubview(pickers)
        header.addArrangedSubview(searchGlass)
        header.axis = .vertical
        header.spacing = 12
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.keyboardDismissMode = .onDrag
        table.estimatedRowHeight = 90
        table.rowHeight = UITableView.automaticDimension
        table.backgroundView = spinner
        table.contentInsetAdjustmentBehavior = .never
        status.font = .preferredFont(forTextStyle: .footnote)
        status.textColor = Palette.secondary
        status.numberOfLines = 0
        var buttonStyle = UIButton.Configuration.glass()
        buttonStyle.baseBackgroundColor = Palette.accent.withAlphaComponent(0.14)
        buttonStyle.baseForegroundColor = Palette.accent
        buttonStyle.cornerStyle = .capsule
        buttonStyle.contentInsets = NSDirectionalEdgeInsets(top: 14, leading: 20, bottom: 14, trailing: 20)
        primary.configuration = buttonStyle
        primary.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            if self.state?.running == true { self.importer?.cancel() }
            else { self.importer?.start(retry: false) }
        }, for: .touchUpInside)
        retry.configuration = .glass()
        retry.setTitle("Retry remaining", for: .normal)
        retry.addAction(UIAction { [weak self] _ in self?.importer?.start(retry: true) }, for: .touchUpInside)
        let footer = UIStackView(arrangedSubviews: [progress, status, retry, primary])
        footer.axis = .vertical
        footer.spacing = 10
        footer.translatesAutoresizingMaskIntoConstraints = false
        footerGlass.contentView.addSubview(footer)
        for child in [table, header, footerGlass] { child.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(child) }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            header.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            header.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            table.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            table.leadingAnchor.constraint(equalTo: view.leadingAnchor), table.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            table.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
            footerGlass.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            footerGlass.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            footerGlass.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor, constant: -12),
            footer.leadingAnchor.constraint(equalTo: footerGlass.contentView.leadingAnchor, constant: 14),
            footer.trailingAnchor.constraint(equalTo: footerGlass.contentView.trailingAnchor, constant: -14),
            footer.topAnchor.constraint(equalTo: footerGlass.contentView.topAnchor, constant: 14),
            footer.bottomAnchor.constraint(equalTo: footerGlass.contentView.bottomAnchor, constant: -14),
        ])
        importer = app.client?.sessionImport(listener: bridge)
        if let snapshot = importer?.snapshot() { receive(snapshot) }
        observation = app.observe { [weak self] in self?.importer?.updateHosts() }
    }
    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        let insets = UIEdgeInsets(top: header.frame.height + 20, left: 0, bottom: footerGlass.frame.height + 24, right: 0)
        if table.contentInset != insets {
            table.contentInset = insets
            table.verticalScrollIndicatorInsets = insets
        }
        if !didSetInsets {
            table.contentOffset.y = -insets.top
            didSetInsets = true
        }
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
    func searchBar(_ searchBar: UISearchBar, textDidChange searchText: String) { importer?.setQuery(query: searchText) }

    private func updatePicker(_ button: UIButton, title: String, image: UIImage?) {
        var config = UIButton.Configuration.glass()
        config.title = title + "  ⌄"
        config.image = image
        config.imagePadding = 8
        config.baseForegroundColor = Palette.text
        config.contentInsets = NSDirectionalEdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12)
        button.configuration = config
        button.accessibilityLabel = title
    }
    private func agentLabel(_ provider: String?) -> String {
        switch provider {
        case "codex": return "Codex"
        case "claude-code": return "Claude Code"
        case "dsh": return "DeepSeek Harness"
        case "pi": return "Pi"
        case "opencode": return "OpenCode"
        default: return "All agents"
        }
    }

    fileprivate func receive(_ snapshot: SessionImportSnapshot) {
        guard isViewLoaded else { return }
        let deviceChanged = state?.deviceId != snapshot.deviceId
        state = snapshot
        let s = snapshot
        if deviceChanged { search.text = s.query }
        notices = [s.error, s.discoveryError].compactMap { $0 } + s.sourceErrors
        if s.deviceId == nil {
            notices.append(s.hosts.isEmpty ? "No computers connected" : "No compatible computer online")
        } else if s.hosts.first(where: { $0.id == s.deviceId })?.online == false {
            notices.append("Computer offline")
        }
        updatePicker(hostButton, title: s.hosts.first(where: { $0.id == s.deviceId })?.name ?? "Computer", image: UIImage(systemName: "laptopcomputer"))
        hostButton.menu = UIMenu(children: s.hosts.map { host in
            UIAction(title: host.name + (!host.supported ? " · Update required" : !host.online ? " · Offline" : ""),
                     attributes: host.online && host.supported ? [] : .disabled,
                     state: host.id == s.deviceId ? .on : .off) { [weak self] _ in self?.importer?.selectDevice(deviceId: host.id) }
        })
        hostButton.isEnabled = !s.running
        navigationItem.rightBarButtonItem?.isEnabled = !s.running && !s.loading
        updatePicker(agents, title: agentLabel(s.provider), image: BrandMarks.image(for: s.provider, side: 18) ?? UIImage(systemName: "square.grid.2x2"))
        let providers: [String?] = [nil, "codex", "claude-code", "dsh", "pi", "opencode"]
        agents.menu = UIMenu(children: providers.map { provider in
            UIAction(title: agentLabel(provider), image: BrandMarks.image(for: provider, side: 20) ?? UIImage(systemName: "square.grid.2x2"),
                     state: s.provider == provider ? .on : .off) { [weak self] _ in self?.importer?.setProvider(provider: provider) }
        })
        agents.isHidden = s.deviceId == nil
        searchGlass.isHidden = s.deviceId == nil
        if s.loading { spinner.startAnimating() } else { spinner.stopAnimating() }
        let done = s.results.filter { $0.phase != .pending && $0.phase != .importing }.count
        let imported = s.results.filter { $0.phase == .imported || $0.phase == .alreadyManaged }.count
        progress.isHidden = !s.running
        progress.progress = s.results.isEmpty ? 0 : Float(done) / Float(s.results.count)
        status.text = s.running ? "\(done) of \(s.results.count) finished" : s.results.isEmpty
            ? "\(s.selectedCount) / \(s.selectionLimit) selected"
            : "\(imported) of \(s.results.count) imported · \(s.selectedCount) selected"
        primary.setTitle(s.running ? (s.cancelling ? "Stopping…" : "Stop import") : (s.selectedCount > 0 ? "Import \(s.selectedCount) selected" : "Import"), for: .normal)
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
                content.text = !s.query.isEmpty || s.provider != nil ? "No matching sessions" : s.hasMore ? "No unimported sessions on this page" : s.discoveryError != nil ? "Discovery failed" : "No unimported sessions"
                content.textProperties.color = Palette.secondary
            } else {
                let row = s.rows[indexPath.row]
                content.text = row.title.isEmpty ? "Untitled session" : row.title
                content.image = BrandMarks.image(for: row.provider, side: 20)
                content.imageProperties.tintColor = BrandMark.brandTint(for: row.provider) ?? Palette.text
                content.secondaryText = [row.cwd, row.disabledReason].compactMap { $0 }.joined(separator: "\n")
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
