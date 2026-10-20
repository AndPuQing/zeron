import UIKit

/// Native completion rows above the composer, on a single glass surface.
final class CommandSuggestions: UIView, UITableViewDataSource, UITableViewDelegate {
    var onPick: ((ComposerCommand) -> Void)?
    var context = ""
    var onRetry: (() -> Void)?
    private let card = Glass.surface(radius: 18)
    private let table = UITableView(frame: .zero, style: .plain)
    private var height: NSLayoutConstraint!
    private var commands: [ComposerCommand] = []
    private var status: (title: String, detail: String?, retry: Bool)?
    private var displayedContext = ""
    private var displayedQuery: String?

    override init(frame: CGRect) {
        super.init(frame: frame)
        accessibilityIdentifier = "slash-menu"
        card.translatesAutoresizingMaskIntoConstraints = false
        table.translatesAutoresizingMaskIntoConstraints = false
        table.accessibilityIdentifier = "slash-list"
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.rowHeight = UITableView.automaticDimension
        table.estimatedRowHeight = 58
        table.contentInsetAdjustmentBehavior = .never
        table.scrollsToTop = false
        table.separatorColor = Palette.hairline.withAlphaComponent(0.5)
        table.separatorInset = UIEdgeInsets(top: 0, left: 52, bottom: 0, right: 14)
        table.tableFooterView = UIView()
        table.register(UITableViewCell.self, forCellReuseIdentifier: "command")
        addSubview(card)
        card.contentView.addSubview(table)
        height = heightAnchor.constraint(equalToConstant: 64)
        height.priority = .defaultHigh
        NSLayoutConstraint.activate([
            height,
            card.topAnchor.constraint(equalTo: topAnchor), card.bottomAnchor.constraint(equalTo: bottomAnchor),
            card.leadingAnchor.constraint(equalTo: leadingAnchor), card.trailingAnchor.constraint(equalTo: trailingAnchor),
            table.topAnchor.constraint(equalTo: card.contentView.topAnchor, constant: 4),
            table.bottomAnchor.constraint(equalTo: card.contentView.bottomAnchor, constant: -4),
            table.leadingAnchor.constraint(equalTo: card.contentView.leadingAnchor),
            table.trailingAnchor.constraint(equalTo: card.contentView.trailingAnchor),
        ])
        registerForTraitChanges([UITraitPreferredContentSizeCategory.self]) { (self: CommandSuggestions, _) in
            self.table.reloadData()
            self.setNeedsLayout()
        }
    }
    required init?(coder: NSCoder) { fatalError() }

    override func layoutSubviews() {
        super.layoutSubviews()
        table.layoutIfNeeded()
        // Measure native rows rather than guessing from the number of results.
        // The surrounding safe-area constraint can shrink this further.
        let fitted = min(280, max(52, table.contentSize.height + 8))
        if abs(height.constant - fitted) > 0.5 { height.constant = fitted }
    }

    func show(_ catalog: ComposerCatalog?, query: String, error: String? = nil) {
        let resetScroll = displayedQuery != query || displayedContext != context
        displayedContext = context
        displayedQuery = query
        commands = catalog?.commands.filter {
            query.isEmpty || $0.token.dropFirst().localizedCaseInsensitiveContains(query) || $0.description.localizedCaseInsensitiveContains(query)
        } ?? []
        if let warning = error ?? catalog?.warning {
            status = ("Retry loading commands", warning, true)
        } else if commands.isEmpty {
            status = (catalog == nil ? "Loading commands…" : "No matching commands", nil, false)
        } else {
            status = nil
        }
        height.constant = min(280, CGFloat(commands.count + (status == nil ? 0 : 1)) * 58 + 8)
        table.reloadData()
        if resetScroll { table.setContentOffset(.zero, animated: false) }
        setNeedsLayout()
        isHidden = false
    }

    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        commands.count + (status == nil ? 0 : 1)
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "command", for: indexPath)
        cell.backgroundColor = .clear
        var content = UIListContentConfiguration.subtitleCell()
        content.textProperties.font = UIFontMetrics(forTextStyle: .subheadline).scaledFont(for: .systemFont(ofSize: 15, weight: .semibold))
        content.textProperties.numberOfLines = 1
        content.secondaryTextProperties.font = .preferredFont(forTextStyle: .caption1)
        content.secondaryTextProperties.numberOfLines = 1
        content.secondaryTextProperties.color = Palette.secondary
        content.imageProperties.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 18, weight: .regular)
        content.imageProperties.reservedLayoutSize = CGSize(width: 24, height: 24)
        content.imageToTextPadding = 12
        content.textToSecondaryTextVerticalPadding = 3
        content.directionalLayoutMargins = NSDirectionalEdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14)
        let enabled: Bool
        if indexPath.row < commands.count {
            let command = commands[indexPath.row]
            enabled = command.unavailableReason == nil
            content.text = command.token
            content.secondaryText = command.unavailableReason ?? (command.description.isEmpty ? command.inputHint : command.description)
            content.image = UIImage(systemName: symbol(for: command.token))
            cell.accessibilityIdentifier = "slash-\(command.token)"
            cell.accessibilityLabel = "\(command.token). \(content.secondaryText ?? "")"
            cell.accessibilityHint = command.unavailableReason == nil ? command.inputHint : nil
        } else if let status {
            enabled = status.retry
            content.text = status.title
            content.secondaryText = status.detail
            content.image = UIImage(systemName: status.retry ? "arrow.clockwise" : "magnifyingglass")
            cell.accessibilityIdentifier = "slash-status"
            cell.accessibilityLabel = [status.title, status.detail].compactMap { $0 }.joined(separator: ". ")
            cell.accessibilityHint = nil
        } else {
            enabled = false
        }
        content.textProperties.color = enabled ? Palette.text : Palette.tertiary
        content.imageProperties.tintColor = enabled ? Palette.secondary : Palette.tertiary
        cell.contentConfiguration = content
        cell.selectionStyle = enabled ? .default : .none
        cell.accessibilityTraits = enabled ? .button : [.button, .notEnabled]
        return cell
    }

    func tableView(_ tableView: UITableView, willSelectRowAt indexPath: IndexPath) -> IndexPath? {
        guard displayedContext == context else { return nil }
        if indexPath.row < commands.count {
            return commands[indexPath.row].unavailableReason == nil ? indexPath : nil
        }
        return status?.retry == true ? indexPath : nil
    }

    func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        guard displayedContext == context else { return }
        if indexPath.row < commands.count {
            let command = commands[indexPath.row]
            if command.unavailableReason == nil { onPick?(command) }
        } else if status?.retry == true {
            onRetry?()
        }
    }

    private func symbol(for token: String) -> String {
        if token.hasPrefix("$") { return "puzzlepiece.extension" }
        let name = token.hasPrefix("/zeron:") ? String(token.dropFirst(7)) : String(token.dropFirst())
        return switch name {
        case "model": "cpu"
        case "new", "clear": "square.and.pencil"
        case "resume", "agents": "magnifyingglass"
        case "rename": "pencil"
        case "archive": "archivebox"
        case "copy": "doc.on.doc"
        case "side", "btw": "arrowshape.turn.up.right"
        case "fork": "arrow.triangle.branch"
        case "theme": "circle.lefthalf.filled"
        case "compact": "text.alignleft"
        case "review": "checkmark.bubble"
        default: "terminal"
        }
    }
}
