import UIKit

/// Scrollable catalog; unavailable commands retain their explanation.
final class CommandSuggestions: UIView {
    var onPick: ((ComposerCommand) -> Void)?
    var onRetry: (() -> Void)?
    private let card = Glass.surface(radius: 18)
    private let scroll = UIScrollView()
    private let stack = UIStackView()
    private var height: NSLayoutConstraint!

    override init(frame: CGRect) {
        super.init(frame: frame)
        accessibilityIdentifier = "slash-menu"
        card.translatesAutoresizingMaskIntoConstraints = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.axis = .vertical
        addSubview(card)
        card.contentView.addSubview(scroll)
        scroll.addSubview(stack)
        height = heightAnchor.constraint(equalToConstant: 64)
        height.priority = .defaultHigh
        NSLayoutConstraint.activate([
            height,
            card.topAnchor.constraint(equalTo: topAnchor), card.bottomAnchor.constraint(equalTo: bottomAnchor),
            card.leadingAnchor.constraint(equalTo: leadingAnchor), card.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.topAnchor.constraint(equalTo: card.contentView.topAnchor), scroll.bottomAnchor.constraint(equalTo: card.contentView.bottomAnchor),
            scroll.leadingAnchor.constraint(equalTo: card.contentView.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: card.contentView.trailingAnchor),
            stack.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor, constant: -4),
            stack.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
            stack.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }

    func show(_ catalog: ComposerCatalog?, query: String, error: String? = nil) {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        let rows = catalog?.commands.filter {
            query.isEmpty || $0.token.dropFirst().localizedCaseInsensitiveContains(query) || $0.description.localizedCaseInsensitiveContains(query)
        } ?? []
        for command in rows {
            var config = UIButton.Configuration.plain()
            config.title = command.token + (command.inputHint.map { "  \($0)" } ?? "")
            config.subtitle = command.unavailableReason ?? command.description
            config.titleAlignment = .leading
            config.contentInsets = NSDirectionalEdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14)
            config.baseForegroundColor = command.unavailableReason == nil ? Palette.text : Palette.tertiary
            let button = UIButton(configuration: config, primaryAction: UIAction { [weak self] _ in
                self?.onPick?(command)
            })
            button.contentHorizontalAlignment = .leading
            button.isEnabled = command.unavailableReason == nil
            button.accessibilityIdentifier = "slash-\(command.token)"
            button.accessibilityLabel = "\(command.token). \(command.unavailableReason ?? command.description)"
            stack.addArrangedSubview(button)
        }
        let warning = error ?? catalog?.warning
        if rows.isEmpty || warning != nil {
            var config = UIButton.Configuration.plain()
            config.title = warning == nil ? (catalog == nil ? "Loading commands…" : "No matching commands") : "Retry loading commands"
            config.subtitle = warning
            config.titleAlignment = .leading
            config.contentInsets = NSDirectionalEdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14)
            let button = UIButton(configuration: config, primaryAction: UIAction { [weak self] _ in self?.onRetry?() })
            button.isEnabled = warning != nil
            stack.addArrangedSubview(button)
        }
        height.constant = min(280, CGFloat(max(1, stack.arrangedSubviews.count)) * 72 + 8)
        scroll.setContentOffset(.zero, animated: false)
        isHidden = false
    }
}
