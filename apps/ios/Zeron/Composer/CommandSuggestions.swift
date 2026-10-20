import UIKit

/// Desktop completion-row layout, with a native scroll view and touch targets.
final class CommandSuggestions: UIView, UITableViewDataSource, UITableViewDelegate {
    var onPick: ((ComposerCommand) -> Void)?
    var context = ""
    var onRetry: (() -> Void)?
    private let card = Glass.surface(radius: 12)
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
        table.estimatedRowHeight = 44
        table.contentInsetAdjustmentBehavior = .never
        table.scrollsToTop = false
        table.separatorStyle = .none
        table.tableFooterView = UIView()
        table.register(CommandSuggestionCell.self, forCellReuseIdentifier: "command")
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
            table.leadingAnchor.constraint(equalTo: card.contentView.leadingAnchor, constant: 4),
            table.trailingAnchor.constraint(equalTo: card.contentView.trailingAnchor, constant: -4),
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
        height.constant = min(280, CGFloat(commands.count + (status == nil ? 0 : 1)) * 44 + 8)
        table.reloadData()
        if resetScroll { table.setContentOffset(.zero, animated: false) }
        setNeedsLayout()
        isHidden = false
    }

    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        commands.count + (status == nil ? 0 : 1)
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "command", for: indexPath) as! CommandSuggestionCell
        let enabled: Bool
        if indexPath.row < commands.count {
            let command = commands[indexPath.row]
            enabled = command.unavailableReason == nil
            // Match desktop: description, unavailability and input hint share
            // the muted inline detail, while the invocation stays canonical.
            var details = [command.description, command.unavailableReason].compactMap { $0 }.filter { !$0.isEmpty }
            if let hint = command.inputHint, !hint.isEmpty { details.append("<\(hint)>") }
            let detail = details.joined(separator: " · ")
            cell.configure(title: command.token, detail: detail,
                           image: UIImage(named: command.token.hasPrefix("$") ? "tool-magic-stick-3" : "tool-command"),
                           enabled: enabled)
            cell.accessibilityIdentifier = "slash-\(command.token)"
            cell.accessibilityLabel = "\(command.token). \(detail)"
            cell.accessibilityHint = command.unavailableReason == nil ? command.inputHint : nil
        } else if let status {
            enabled = status.retry
            cell.configure(title: status.title, detail: status.detail ?? "",
                           image: status.retry ? UIImage(systemName: "arrow.clockwise") : UIImage(named: "tool-command"),
                           enabled: enabled)
            cell.accessibilityIdentifier = "slash-status"
            cell.accessibilityLabel = [status.title, status.detail].compactMap { $0 }.joined(separator: ". ")
            cell.accessibilityHint = nil
        } else {
            enabled = false
        }
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

}

/// Mirrors `popover::completion_row_content`: a 16-point icon, medium label
/// and muted inline detail, separated by 8 points. The title takes at most
/// 55% of the row when a detail is present, as on desktop.
private final class CommandSuggestionCell: UITableViewCell {
    private let icon = UIImageView()
    private let title = UILabel()
    private let detail = UILabel()
    private var titleLimit: NSLayoutConstraint!

    override init(style: UITableViewCell.CellStyle, reuseIdentifier: String?) {
        super.init(style: style, reuseIdentifier: reuseIdentifier)
        backgroundColor = .clear
        isAccessibilityElement = true
        icon.contentMode = .scaleAspectFit
        for label in [title, detail] {
            label.numberOfLines = 1
            label.lineBreakMode = .byTruncatingTail
            label.adjustsFontForContentSizeCategory = true
            label.isAccessibilityElement = false
        }
        title.accessibilityIdentifier = "slash-title"
        detail.accessibilityIdentifier = "slash-detail"
        title.font = UIFontMetrics(forTextStyle: .subheadline).scaledFont(for: Fonts.ui(.sansMedium, 13))
        detail.font = UIFontMetrics(forTextStyle: .subheadline).scaledFont(for: Fonts.ui(.sans, 12.5))
        title.setContentHuggingPriority(.defaultHigh, for: .horizontal)
        detail.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let row = UIStackView(arrangedSubviews: [icon, title, detail])
        row.alignment = .center
        row.spacing = 8
        row.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(row)
        titleLimit = title.widthAnchor.constraint(lessThanOrEqualTo: row.widthAnchor, multiplier: 0.55)
        NSLayoutConstraint.activate([
            contentView.heightAnchor.constraint(greaterThanOrEqualToConstant: 44),
            row.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 8),
            row.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -8),
            row.topAnchor.constraint(equalTo: contentView.topAnchor, constant: 6),
            row.bottomAnchor.constraint(equalTo: contentView.bottomAnchor, constant: -6),
            icon.widthAnchor.constraint(equalToConstant: 16),
            icon.heightAnchor.constraint(equalToConstant: 16),
        ])
        let highlight = UIView()
        highlight.backgroundColor = Palette.controlFill
        highlight.layer.cornerRadius = 7
        selectedBackgroundView = highlight
    }
    required init?(coder: NSCoder) { fatalError() }

    func configure(title text: String, detail description: String, image: UIImage?, enabled: Bool) {
        title.text = text
        detail.text = description
        detail.isHidden = description.isEmpty
        titleLimit.isActive = !description.isEmpty
        icon.image = image
        icon.tintColor = Palette.secondary
        title.textColor = Palette.text
        detail.textColor = Palette.secondary
        contentView.alpha = enabled ? 1 : 0.5
    }
}
