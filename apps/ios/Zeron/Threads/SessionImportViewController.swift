import UIKit

/// Remote history selection; the Rust controller owns discovery and batch lifetimes.
final class SessionImportViewController: UIViewController, UITableViewDataSource, UITableViewDelegate, UITextFieldDelegate, UIAdaptivePresentationControllerDelegate {
    private enum Section: Equatable { case notices, results, sessions, unavailable, actions }
    private enum ListAction: Equatable { case selectVisible, loadMore }
    private let app: AppModel
    private var importer: CoreSessionImport?
    private var state: SessionImportSnapshot?
    private var observation: AnyObject?
    private lazy var bridge = ImportListenerBridge(owner: self)
    private let hostButton = UIButton(type: .system)
    private let agents = UIButton(type: .system)
    private let search = UITextField()
    private let table = UITableView(frame: .zero, style: .insetGrouped)
    private let status = UILabel()
    private let primary = UIButton(type: .system)
    private let retry = UIButton(type: .system)
    private let progress = UIProgressView(progressViewStyle: .default)
    private let spinner = UIActivityIndicatorView(style: .medium)
    private let header = UIStackView()
    private let searchGlass = Glass.surface()
    private let footer = UIStackView()
    private var sections: [Section] = []
    private var actions: [ListAction] = []
    private var availableRows: [SessionImportRow] = []
    private var unavailableRows: [SessionImportRow] = []
    private var showsUnavailable = false
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
        hostButton.changesSelectionAsPrimaryAction = true
        hostButton.accessibilityIdentifier = "import-device"
        hostButton.contentHorizontalAlignment = .leading
        agents.showsMenuAsPrimaryAction = true
        agents.changesSelectionAsPrimaryAction = true
        agents.accessibilityIdentifier = "import-agents"
        search.placeholder = "Search sessions"
        search.font = .preferredFont(forTextStyle: .body)
        search.adjustsFontForContentSizeCategory = true
        search.textColor = Palette.text
        search.clearButtonMode = .whileEditing
        search.returnKeyType = .search
        search.autocapitalizationType = .none
        search.autocorrectionType = .no
        search.accessibilityLabel = "Search sessions"
        search.accessibilityIdentifier = "import-search"
        search.delegate = self
        search.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            self.importer?.setQuery(query: self.search.text ?? "")
        }, for: .editingChanged)
        let pickers = UIStackView(arrangedSubviews: [hostButton, agents])
        pickers.distribution = .fillEqually
        pickers.spacing = 10
        let searchIcon = UIImageView(image: UIImage(systemName: "magnifyingglass"))
        searchIcon.tintColor = Palette.secondary
        searchIcon.setContentHuggingPriority(.required, for: .horizontal)
        let searchContents = UIStackView(arrangedSubviews: [searchIcon, search])
        searchContents.alignment = .center
        searchContents.spacing = 8
        searchContents.translatesAutoresizingMaskIntoConstraints = false
        searchGlass.contentView.addSubview(searchContents)
        NSLayoutConstraint.activate([
            hostButton.heightAnchor.constraint(greaterThanOrEqualToConstant: Glass.Metrics.control),
            agents.heightAnchor.constraint(greaterThanOrEqualToConstant: Glass.Metrics.control),
            searchGlass.heightAnchor.constraint(greaterThanOrEqualToConstant: Glass.Metrics.control),
            searchContents.leadingAnchor.constraint(equalTo: searchGlass.contentView.leadingAnchor, constant: 14),
            searchContents.trailingAnchor.constraint(equalTo: searchGlass.contentView.trailingAnchor, constant: -14),
            searchContents.topAnchor.constraint(equalTo: searchGlass.contentView.topAnchor, constant: 10),
            searchContents.bottomAnchor.constraint(equalTo: searchGlass.contentView.bottomAnchor, constant: -10),
        ])
        header.addArrangedSubview(pickers)
        header.addArrangedSubview(searchGlass)
        header.axis = .vertical
        header.spacing = 8
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.keyboardDismissMode = .onDrag
        table.estimatedRowHeight = 64
        table.rowHeight = UITableView.automaticDimension
        table.backgroundView = spinner
        table.contentInsetAdjustmentBehavior = .never
        table.contentInset.bottom = 8
        table.sectionHeaderTopPadding = 0
        table.accessibilityIdentifier = "import-list"
        status.font = .preferredFont(forTextStyle: .footnote)
        status.textColor = Palette.secondary
        status.adjustsFontForContentSizeCategory = true
        status.numberOfLines = 2
        status.accessibilityIdentifier = "import-status"
        status.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        var buttonStyle = UIButton.Configuration.prominentGlass()
        buttonStyle.baseBackgroundColor = Palette.accent
        buttonStyle.baseForegroundColor = .white
        buttonStyle.cornerStyle = .capsule
        buttonStyle.contentInsets = NSDirectionalEdgeInsets(top: 12, leading: 20, bottom: 12, trailing: 20)
        primary.configuration = buttonStyle
        primary.accessibilityIdentifier = "import-primary"
        primary.setContentCompressionResistancePriority(.required, for: .horizontal)
        primary.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            if self.state?.running == true { self.importer?.cancel() }
            else { self.importer?.start(retry: false) }
        }, for: .touchUpInside)
        retry.configuration = .plain()
        retry.setTitle("Retry remaining", for: .normal)
        retry.addAction(UIAction { [weak self] _ in self?.importer?.start(retry: true) }, for: .touchUpInside)
        let selection = UIStackView(arrangedSubviews: [status, primary])
        selection.alignment = .center
        selection.spacing = 12
        footer.addArrangedSubview(progress)
        footer.addArrangedSubview(selection)
        footer.addArrangedSubview(retry)
        footer.axis = .vertical
        footer.spacing = 6
        for child in [table, header, footer] { child.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(child) }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            header.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            header.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            table.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 8),
            table.leadingAnchor.constraint(equalTo: view.leadingAnchor), table.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            table.bottomAnchor.constraint(equalTo: footer.topAnchor, constant: -8),
            footer.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 20),
            footer.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -20),
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
    func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        textField.resignFirstResponder()
        return true
    }

    private func updatePicker(_ button: UIButton, title: String, image: UIImage?) {
        var config = UIButton.Configuration.glass()
        config.title = title
        config.indicator = .popup
        config.image = image
        config.preferredSymbolConfigurationForImage = UIImage.SymbolConfiguration(pointSize: 17, weight: .regular)
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
        case "copilot": return "GitHub Copilot"
        case "cursor": return "Cursor"
        case "grok": return "Grok"
        case "hermes": return "Hermes"
        case "dsh": return "DeepSeek Harness"
        case "pi": return "Pi"
        case "omp": return "Oh My Pi"
        case "opencode": return "OpenCode"
        default: return "All agents"
        }
    }

    fileprivate func receive(_ snapshot: SessionImportSnapshot) {
        guard isViewLoaded else { return }
        let deviceChanged = state?.deviceId != snapshot.deviceId
        state = snapshot
        let s = snapshot
        if deviceChanged {
            search.text = s.query
            showsUnavailable = false
        }
        availableRows = s.rows.filter { $0.disabledReason == nil }
        unavailableRows = s.rows.filter { $0.disabledReason != nil }
        notices = [s.error, s.discoveryError].compactMap { $0 } + s.sourceErrors
        if s.deviceId == nil {
            notices.append(s.hosts.isEmpty ? "No computers connected" : "No compatible computer online")
        } else if s.hosts.first(where: { $0.id == s.deviceId })?.online == false {
            notices.append("Computer offline")
        }
        updatePicker(hostButton, title: s.hosts.first(where: { $0.id == s.deviceId })?.name ?? "Computer", image: UIImage(systemName: "laptopcomputer"))
        hostButton.menu = UIMenu(options: .singleSelection, children: s.hosts.map { host in
            UIAction(title: host.name + (!host.supported ? " · Update required" : !host.online ? " · Offline" : ""),
                     attributes: host.online && host.supported ? [] : .disabled,
                     state: host.id == s.deviceId ? .on : .off) { [weak self] _ in self?.importer?.selectDevice(deviceId: host.id) }
        })
        hostButton.isEnabled = !s.running
        navigationItem.rightBarButtonItem?.isEnabled = !s.running && !s.loading
        updatePicker(agents, title: agentLabel(s.provider), image: BrandMarks.image(for: s.provider, side: 18))
        let providers: [String?] = [nil, "codex", "claude-code", "copilot", "cursor", "grok", "hermes", "dsh", "pi", "omp", "opencode"]
        agents.menu = UIMenu(options: .singleSelection, children: providers.map { provider in
            UIAction(title: agentLabel(provider), image: BrandMarks.image(for: provider, side: 18),
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
            ? (s.selectedCount == 0 ? "Select sessions" : "\(s.selectedCount) selected")
            : "\(imported) of \(s.results.count) imported · \(s.selectedCount) selected"
        primary.accessibilityHint = "Select up to \(s.selectionLimit) sessions per import."
        primary.setTitle(s.running ? (s.cancelling ? "Stopping…" : "Stop import") : (s.selectedCount > 0 ? "Import \(s.selectedCount)" : "Import"), for: .normal)
        primary.isEnabled = s.running ? !s.cancelling : s.selectedCount > 0 && !s.loading
        retry.isHidden = s.running || !s.results.contains(where: { $0.retryable })
        sections = []
        if !notices.isEmpty { sections.append(.notices) }
        if !s.results.isEmpty { sections.append(.results) }
        if s.deviceId != nil {
            sections.append(.sessions)
            if !unavailableRows.isEmpty { sections.append(.unavailable) }
        }
        actions = []
        if !availableRows.isEmpty { actions.append(.selectVisible) }
        if s.hasMore { actions.append(.loadMore) }
        if !actions.isEmpty { sections.append(.actions) }
        table.reloadData()
    }
    func numberOfSections(in tableView: UITableView) -> Int { sections.count }
    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        guard let s = state else { return 0 }
        switch sections[section] {
        case .notices: return notices.count + (s.discoveryError != nil && !s.running ? 1 : 0)
        case .results: return s.results.count
        case .sessions: return availableRows.isEmpty && !s.loading ? 1 : availableRows.count
        case .unavailable: return 1 + (showsUnavailable ? unavailableRows.count : 0)
        case .actions: return actions.count
        }
    }
    func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        switch sections[section] {
        case .results: return "Import results"
        case .sessions: return "Not imported"
        default: return nil
        }
    }
    func tableView(_ tableView: UITableView, heightForHeaderInSection section: Int) -> CGFloat {
        self.tableView(tableView, titleForHeaderInSection: section) == nil ? .leastNormalMagnitude : UITableView.automaticDimension
    }
    func tableView(_ tableView: UITableView, heightForFooterInSection section: Int) -> CGFloat { 8 }
    private func emptyMessage(_ s: SessionImportSnapshot) -> String {
        if !unavailableRows.isEmpty { return "No importable sessions on this page" }
        if !s.query.isEmpty || s.provider != nil { return "No matching sessions" }
        if s.hasMore { return "No unimported sessions on this page" }
        if s.discoveryError != nil { return "Discovery failed" }
        return "No unimported sessions"
    }
    private func configureSession(_ row: SessionImportRow, cell: UITableViewCell, content: inout UIListContentConfiguration) {
        content.text = row.title.isEmpty ? "Untitled session" : row.title
        content.textProperties.numberOfLines = 1
        content.image = BrandMarks.image(for: row.provider, side: 20)
        content.imageProperties.tintColor = BrandMark.brandTint(for: row.provider) ?? Palette.text
        content.secondaryText = [row.cwd, row.disabledReason].compactMap { $0 }.joined(separator: "\n")
        content.secondaryTextProperties.numberOfLines = row.disabledReason == nil ? 1 : 2
        content.secondaryTextProperties.lineBreakMode = .byTruncatingMiddle
        content.textProperties.color = row.disabledReason == nil ? Palette.text : Palette.secondary
        cell.accessibilityIdentifier = "import-session-\(row.sourceRef)"
        cell.accessibilityTraits = row.disabledReason == nil ? [.button] : [.notEnabled]
        if row.disabledReason == nil {
            let mark = UIImageView(image: UIImage(systemName: row.selected ? "checkmark.circle.fill" : "circle",
                withConfiguration: UIImage.SymbolConfiguration(pointSize: 22)))
            mark.tintColor = row.selected ? Palette.accent : Palette.tertiary
            cell.accessoryView = mark
            cell.selectionStyle = state?.running == false && state?.loading == false ? .default : .none
        }
        if row.selected { cell.accessibilityTraits.insert(.selected) }
    }
    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "import") ?? UITableViewCell(style: .subtitle, reuseIdentifier: "import")
        guard let s = state else { return cell }
        var content = cell.defaultContentConfiguration()
        content.textProperties.numberOfLines = 2
        content.secondaryTextProperties.numberOfLines = 0
        content.secondaryTextProperties.color = Palette.secondary
        cell.accessoryType = .none
        cell.accessoryView = nil
        cell.accessibilityIdentifier = nil
        cell.accessibilityValue = nil
        cell.selectionStyle = .none
        cell.accessibilityTraits = .staticText
        switch sections[indexPath.section] {
        case .notices:
            if indexPath.row < notices.count { content.text = notices[indexPath.row]; content.textProperties.color = Palette.secondary }
            else { content.text = "Retry discovery"; content.textProperties.color = Palette.accent; cell.accessibilityTraits = .button }
        case .results:
            let result = s.results[indexPath.row]
            content.text = result.title
            content.secondaryText = result.reason ?? phaseLabel(result.phase)
            content.secondaryTextProperties.color = result.phase == .failed ? .systemRed : Palette.secondary
        case .sessions:
            if availableRows.isEmpty {
                content.text = emptyMessage(s)
                content.textProperties.color = Palette.secondary
            } else {
                configureSession(availableRows[indexPath.row], cell: cell, content: &content)
            }
        case .unavailable:
            if indexPath.row == 0 {
                content.text = "\(unavailableRows.count) unavailable sessions"
                content.textProperties.color = Palette.secondary
                content.image = UIImage(systemName: showsUnavailable ? "chevron.down" : "chevron.right",
                    withConfiguration: UIImage.SymbolConfiguration(pointSize: 12, weight: .semibold))
                content.imageProperties.tintColor = Palette.tertiary
                cell.accessibilityIdentifier = "import-unavailable"
                cell.accessibilityValue = showsUnavailable ? "Expanded" : "Collapsed"
                cell.accessibilityTraits = .button
            } else {
                configureSession(unavailableRows[indexPath.row - 1], cell: cell, content: &content)
            }
        case .actions:
            let action = actions[indexPath.row]
            content.text = action == .selectVisible ? "Select visible" : "Load more sessions"
            cell.accessibilityIdentifier = action == .selectVisible ? "import-select-visible" : "import-load-more"
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
        guard let s = state else { return }
        if sections[indexPath.section] == .unavailable && indexPath.row == 0 {
            showsUnavailable.toggle()
            table.reloadData()
            return
        }
        guard !s.running, !s.loading else { return }
        switch sections[indexPath.section] {
        case .notices:
            if indexPath.row == notices.count { if s.hasMore { importer?.loadMore() } else { importer?.refresh() } }
        case .sessions:
            if !availableRows.isEmpty { importer?.toggle(sourceRef: availableRows[indexPath.row].sourceRef) }
        case .actions:
            switch actions[indexPath.row] {
            case .selectVisible: importer?.selectVisible()
            case .loadMore: importer?.loadMore()
            }
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
