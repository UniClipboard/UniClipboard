import UIKit

@MainActor
final class KeyboardRailView: UIView {
  enum SyncIndicatorState: Equatable {
    case idle
    case syncing
    case error
  }

  var onSelectFilter: ((KeyboardCategoryFilter) -> Void)?
  var onTapRefresh: (() -> Void)?

  private let container = UIView()
  private let blurView = UIVisualEffectView(effect: UIBlurEffect(style: .systemChromeMaterial))
  private let stack = UIStackView()
  private var filterButtons: [KeyboardCategoryFilter: RailItemButton] = [:]
  private let refreshButton = RailItemButton()
  private let separator = UIView()
  private var selectedFilter: KeyboardCategoryFilter = .all
  private var syncState: SyncIndicatorState = .idle

  override init(frame: CGRect) {
    super.init(frame: frame)
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  func configure(
    selectedFilter: KeyboardCategoryFilter,
    pinnedCount: Int,
    syncState: SyncIndicatorState,
    strings: KeyboardStrings
  ) {
    self.selectedFilter = selectedFilter
    self.syncState = syncState

    filterButtons[.all]?.configure(
      symbol: "square.grid.2x2",
      title: strings.railAll,
      selected: selectedFilter == .all,
      badgeCount: 0,
      statusDotColor: nil
    )
    filterButtons[.pinned]?.configure(
      symbol: "pin",
      title: strings.railPinned,
      selected: selectedFilter == .pinned,
      badgeCount: pinnedCount,
      statusDotColor: nil
    )
    filterButtons[.url]?.configure(
      symbol: "link",
      title: strings.railLinks,
      selected: selectedFilter == .url,
      badgeCount: 0,
      statusDotColor: nil
    )
    filterButtons[.image]?.configure(
      symbol: "photo",
      title: strings.railImages,
      selected: selectedFilter == .image,
      badgeCount: 0,
      statusDotColor: nil
    )

    let dotColor: UIColor?
    let refreshTitle: String
    switch syncState {
    case .idle:
      dotColor = KeyboardTheme.tintGreen
      refreshTitle = strings.railSync
    case .syncing:
      dotColor = nil
      refreshTitle = strings.railSync
    case .error:
      dotColor = KeyboardTheme.tintAmber
      refreshTitle = strings.railRetry
    }
    refreshButton.configure(
      symbol: "arrow.clockwise",
      title: refreshTitle,
      selected: false,
      badgeCount: 0,
      statusDotColor: dotColor
    )
    refreshButton.setSpinning(syncState == .syncing)
  }

  private func buildHierarchy() {
    translatesAutoresizingMaskIntoConstraints = false

    container.translatesAutoresizingMaskIntoConstraints = false
    container.backgroundColor = KeyboardTheme.railBackground
    container.layer.cornerRadius = 16
    container.layer.cornerCurve = .continuous
    container.layer.borderWidth = 0.5
    container.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    container.clipsToBounds = true
    addSubview(container)

    blurView.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(blurView)

    stack.axis = .vertical
    stack.alignment = .center
    stack.spacing = 3
    stack.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(stack)

    for filter in KeyboardCategoryFilter.allCases {
      let button = RailItemButton()
      button.tag = filter.rawValue
      button.addTarget(self, action: #selector(handleFilterTap(_:)), for: .touchUpInside)
      filterButtons[filter] = button
      stack.addArrangedSubview(button)
    }

    let spacer = UIView()
    spacer.translatesAutoresizingMaskIntoConstraints = false
    spacer.setContentHuggingPriority(.defaultLow, for: .vertical)
    stack.addArrangedSubview(spacer)

    separator.translatesAutoresizingMaskIntoConstraints = false
    separator.backgroundColor = KeyboardTheme.separator
    stack.addArrangedSubview(separator)

    refreshButton.addTarget(self, action: #selector(handleRefreshTap), for: .touchUpInside)
    stack.addArrangedSubview(refreshButton)

    NSLayoutConstraint.activate([
      widthAnchor.constraint(equalToConstant: KeyboardLayoutMetrics.railWidth),
      container.leadingAnchor.constraint(equalTo: leadingAnchor),
      container.trailingAnchor.constraint(equalTo: trailingAnchor),
      container.topAnchor.constraint(equalTo: topAnchor),
      container.bottomAnchor.constraint(equalTo: bottomAnchor),

      blurView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
      blurView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
      blurView.topAnchor.constraint(equalTo: container.topAnchor),
      blurView.bottomAnchor.constraint(equalTo: container.bottomAnchor),

      stack.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 4),
      stack.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -4),
      stack.topAnchor.constraint(equalTo: container.topAnchor, constant: 4),
      stack.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -4),

      separator.widthAnchor.constraint(equalToConstant: 24),
      separator.heightAnchor.constraint(equalToConstant: 0.5),
    ])
  }

  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    container.layer.borderColor = KeyboardTheme.cardBorder.cgColor
  }

  @objc private func handleFilterTap(_ sender: UIButton) {
    guard let filter = KeyboardCategoryFilter(rawValue: sender.tag) else { return }
    onSelectFilter?(filter)
  }

  @objc private func handleRefreshTap() {
    onTapRefresh?()
  }
}

@MainActor
private final class RailItemButton: UIControl {
  private let iconView = UIImageView()
  private let titleLabel = UILabel()
  private let badgeLabel = UILabel()
  private let statusDot = UIView()
  private var isCurrentlySelected = false

  override init(frame: CGRect) {
    super.init(frame: frame)
    translatesAutoresizingMaskIntoConstraints = false
    layer.cornerRadius = 11
    layer.cornerCurve = .continuous

    iconView.translatesAutoresizingMaskIntoConstraints = false
    iconView.contentMode = .scaleAspectFit
    iconView.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 13.5, weight: .semibold)

    titleLabel.translatesAutoresizingMaskIntoConstraints = false
    titleLabel.font = .systemFont(ofSize: 9, weight: .semibold)
    titleLabel.textAlignment = .center

    badgeLabel.translatesAutoresizingMaskIntoConstraints = false
    badgeLabel.font = .systemFont(ofSize: 8.5, weight: .bold)
    badgeLabel.textAlignment = .center
    badgeLabel.layer.cornerRadius = 6.5
    badgeLabel.clipsToBounds = true
    badgeLabel.isHidden = true

    statusDot.translatesAutoresizingMaskIntoConstraints = false
    statusDot.layer.cornerRadius = 3
    statusDot.layer.borderWidth = 1.2
    statusDot.layer.borderColor = KeyboardTheme.cardBackground.cgColor
    statusDot.isHidden = true

    let stack = UIStackView(arrangedSubviews: [iconView, titleLabel])
    stack.axis = .vertical
    stack.alignment = .center
    stack.spacing = 2
    stack.isUserInteractionEnabled = false
    stack.translatesAutoresizingMaskIntoConstraints = false
    addSubview(stack)
    addSubview(badgeLabel)
    addSubview(statusDot)

    NSLayoutConstraint.activate([
      widthAnchor.constraint(equalToConstant: KeyboardLayoutMetrics.railButtonSize.width),
      heightAnchor.constraint(equalToConstant: KeyboardLayoutMetrics.railButtonSize.height),
      stack.centerXAnchor.constraint(equalTo: centerXAnchor),
      stack.centerYAnchor.constraint(equalTo: centerYAnchor),
      iconView.heightAnchor.constraint(equalToConstant: 16),

      badgeLabel.topAnchor.constraint(equalTo: topAnchor, constant: 2),
      badgeLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
      badgeLabel.widthAnchor.constraint(greaterThanOrEqualToConstant: 13),
      badgeLabel.heightAnchor.constraint(equalToConstant: 13),

      statusDot.topAnchor.constraint(equalTo: topAnchor, constant: 5),
      statusDot.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -5),
      statusDot.widthAnchor.constraint(equalToConstant: 6),
      statusDot.heightAnchor.constraint(equalToConstant: 6),
    ])
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  override var isHighlighted: Bool {
    didSet {
      UIView.animate(withDuration: 0.1) {
        self.transform = self.isHighlighted ? CGAffineTransform(scaleX: 0.93, y: 0.93) : .identity
        if !self.isCurrentlySelected {
          self.backgroundColor = self.isHighlighted ? KeyboardTheme.fill : .clear
        }
      }
    }
  }

  func configure(
    symbol: String,
    title: String,
    selected: Bool,
    badgeCount: Int,
    statusDotColor: UIColor?
  ) {
    isCurrentlySelected = selected
    iconView.image = UIImage(systemName: symbol)
    titleLabel.text = title
    accessibilityLabel = title
    accessibilityTraits = selected ? [.button, .selected] : [.button]

    backgroundColor = selected ? KeyboardTheme.accent : .clear
    let fg: UIColor = selected ? KeyboardTheme.onAccent : .secondaryLabel
    iconView.tintColor = fg
    titleLabel.textColor = fg

    if badgeCount > 0 {
      badgeLabel.isHidden = false
      badgeLabel.text = badgeCount > 9 ? "9+" : "\(badgeCount)"
      badgeLabel.backgroundColor = selected ? KeyboardTheme.onAccent : KeyboardTheme.tintPin
      badgeLabel.textColor = selected ? KeyboardTheme.accent : .white
    } else {
      badgeLabel.isHidden = true
    }

    if let dotColor = statusDotColor {
      statusDot.isHidden = false
      statusDot.backgroundColor = dotColor
      statusDot.layer.borderColor = KeyboardTheme.cardBackground.cgColor
    } else {
      statusDot.isHidden = true
    }
  }

  func setSpinning(_ spinning: Bool) {
    let key = "unic.spin"
    if spinning {
      guard iconView.layer.animation(forKey: key) == nil else { return }
      let anim = CABasicAnimation(keyPath: "transform.rotation.z")
      anim.toValue = Double.pi * 2
      anim.duration = 0.85
      anim.repeatCount = .infinity
      iconView.layer.add(anim, forKey: key)
    } else {
      iconView.layer.removeAnimation(forKey: key)
    }
  }
}
