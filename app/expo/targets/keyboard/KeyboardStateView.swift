import UIKit

@MainActor
final class KeyboardStateView: UIView {
  private let container = UIView()
  private let iconBox = UIView()
  private let iconView = UIImageView()
  private let titleLabel = UILabel()
  private let messageLabel = UILabel()
  private let chipsStack = UIStackView()
  private let actionButton = UIButton(type: .system)
  private var actionHandler: (() -> Void)?

  override init(frame: CGRect) {
    super.init(frame: frame)
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  func configure(
    symbol: String,
    symbolColor: UIColor = .label,
    title: String,
    message: String,
    chips: [String] = [],
    actionTitle: String? = nil,
    action: (() -> Void)? = nil
  ) {
    iconView.image = UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 18, weight: .semibold))
    iconView.tintColor = symbolColor
    titleLabel.text = title
    messageLabel.text = message

    chipsStack.arrangedSubviews.forEach {
      chipsStack.removeArrangedSubview($0)
      $0.removeFromSuperview()
    }
    chipsStack.isHidden = chips.isEmpty
    for text in chips {
      let pill = PaddingPillLabel()
      pill.text = text
      pill.font = .systemFont(ofSize: 10.5, weight: .medium)
      pill.textColor = .secondaryLabel
      pill.backgroundColor = KeyboardTheme.fill
      chipsStack.addArrangedSubview(pill)
    }

    actionHandler = action
    if let actionTitle {
      actionButton.isHidden = false
      actionButton.setTitle(actionTitle, for: .normal)
    } else {
      actionButton.isHidden = true
    }
  }

  private func buildHierarchy() {
    translatesAutoresizingMaskIntoConstraints = false

    container.translatesAutoresizingMaskIntoConstraints = false
    container.backgroundColor = KeyboardTheme.railBackground
    container.layer.cornerRadius = 16
    container.layer.cornerCurve = .continuous
    container.layer.borderWidth = 0.5
    container.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    addSubview(container)

    iconBox.translatesAutoresizingMaskIntoConstraints = false
    iconBox.backgroundColor = KeyboardTheme.cardBackground
    iconBox.layer.cornerRadius = 13
    iconBox.layer.cornerCurve = .continuous
    iconBox.layer.borderWidth = 0.5
    iconBox.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    iconView.translatesAutoresizingMaskIntoConstraints = false
    iconView.contentMode = .scaleAspectFit
    iconBox.addSubview(iconView)

    titleLabel.font = .systemFont(ofSize: 14, weight: .semibold)
    titleLabel.textColor = .label
    titleLabel.textAlignment = .center
    titleLabel.numberOfLines = 2

    messageLabel.font = .systemFont(ofSize: 11.5)
    messageLabel.textColor = .secondaryLabel
    messageLabel.textAlignment = .center
    messageLabel.numberOfLines = 3

    chipsStack.axis = .horizontal
    chipsStack.alignment = .center
    chipsStack.spacing = 6

    actionButton.translatesAutoresizingMaskIntoConstraints = false
    actionButton.backgroundColor = KeyboardTheme.accent
    actionButton.setTitleColor(KeyboardTheme.onAccent, for: .normal)
    actionButton.titleLabel?.font = .systemFont(ofSize: 12, weight: .semibold)
    actionButton.contentEdgeInsets = UIEdgeInsets(top: 7, left: 16, bottom: 7, right: 16)
    actionButton.layer.cornerRadius = 15
    actionButton.layer.cornerCurve = .continuous
    actionButton.addTarget(self, action: #selector(handleAction), for: .touchUpInside)

    let stack = UIStackView(arrangedSubviews: [iconBox, titleLabel, messageLabel, chipsStack, actionButton])
    stack.axis = .vertical
    stack.alignment = .center
    stack.spacing = 7
    stack.setCustomSpacing(10, after: messageLabel)
    stack.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(stack)

    NSLayoutConstraint.activate([
      container.leadingAnchor.constraint(equalTo: leadingAnchor),
      container.trailingAnchor.constraint(equalTo: trailingAnchor),
      container.topAnchor.constraint(equalTo: topAnchor),
      container.bottomAnchor.constraint(equalTo: bottomAnchor),

      iconBox.widthAnchor.constraint(equalToConstant: 40),
      iconBox.heightAnchor.constraint(equalToConstant: 40),
      iconView.centerXAnchor.constraint(equalTo: iconBox.centerXAnchor),
      iconView.centerYAnchor.constraint(equalTo: iconBox.centerYAnchor),

      stack.leadingAnchor.constraint(greaterThanOrEqualTo: container.leadingAnchor, constant: 16),
      stack.trailingAnchor.constraint(lessThanOrEqualTo: container.trailingAnchor, constant: -16),
      stack.centerXAnchor.constraint(equalTo: container.centerXAnchor),
      stack.centerYAnchor.constraint(equalTo: container.centerYAnchor),
    ])
  }

  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    container.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    iconBox.layer.borderColor = KeyboardTheme.cardBorder.cgColor
  }

  @objc private func handleAction() {
    actionHandler?()
  }
}
