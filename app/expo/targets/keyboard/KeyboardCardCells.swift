import UIKit

/// Full-width single-tap list card (C1 Pure Single-Tap — no Quick-Send button).
@MainActor
final class KeyboardListRowCell: UICollectionViewCell {
  static let reuseIdentifier = "KeyboardListRowCell"

  private let cardContainer = UIView()
  private let kindBar = UIView()
  private let mediaThumb = UIImageView()
  private let metaStack = UIStackView()
  private let latestBadge = PaddingPillLabel()
  private let otpBadge = PaddingPillLabel()
  private let domainLabel = UILabel()
  private let metaLabel = UILabel()
  private let pinIcon = UIImageView()
  private let titleLabel = UILabel()
  private let subtitleLabel = UILabel()
  private let actedBadge = UIView()
  private let actedIcon = UIImageView()

  private var representedID: String?

  override init(frame: CGRect) {
    super.init(frame: frame)
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  override var isHighlighted: Bool {
    didSet {
      UIView.animate(withDuration: 0.1) {
        self.cardContainer.transform = self.isHighlighted ? CGAffineTransform(scaleX: 0.982, y: 0.982) : .identity
      }
    }
  }

  override func prepareForReuse() {
    super.prepareForReuse()
    representedID = nil
    mediaThumb.image = nil
    mediaThumb.isHidden = true
    kindBar.isHidden = false
    actedBadge.isHidden = true
    subtitleLabel.isHidden = true
  }

  func configure(
    item: KeyboardClipItem,
    isLatest: Bool,
    isActed: Bool,
    strings: KeyboardStrings,
    thumbnail: UIImage?
  ) {
    representedID = item.id

    // Card surface styling
    if item.kind == .otp {
      cardContainer.backgroundColor = KeyboardTheme.cardOTPBackground
      cardContainer.layer.borderColor = KeyboardTheme.otpBorder.cgColor
      cardContainer.layer.borderWidth = 1
    } else if isLatest {
      cardContainer.backgroundColor = KeyboardTheme.cardSpotlightBackground
      cardContainer.layer.borderColor = KeyboardTheme.spotlightBorder.cgColor
      cardContainer.layer.borderWidth = 1
    } else {
      cardContainer.backgroundColor = KeyboardTheme.cardBackground
      cardContainer.layer.borderColor = KeyboardTheme.cardBorder.cgColor
      cardContainer.layer.borderWidth = 0.5
    }

    if isActed {
      cardContainer.layer.borderColor = KeyboardTheme.tintGreen.cgColor
      cardContainer.layer.borderWidth = 1.25
    }

    // Left leading indicator vs image thumbnail
    let isImage = item.kind == .image
    kindBar.isHidden = isImage
    mediaThumb.isHidden = !isImage
    if isImage {
      mediaThumb.image = thumbnail ?? UIImage(systemName: "photo")
      mediaThumb.contentMode = thumbnail != nil ? .scaleAspectFill : .center
      mediaThumb.tintColor = .secondaryLabel
    } else {
      kindBar.backgroundColor = KeyboardTheme.kindColor(item.kind)
    }

    // Meta line
    latestBadge.isHidden = !isLatest
    latestBadge.text = strings.badgeLatest
    latestBadge.backgroundColor = KeyboardTheme.accent
    latestBadge.textColor = KeyboardTheme.onAccent

    otpBadge.isHidden = item.kind != .otp
    otpBadge.text = strings.badgeOTP
    otpBadge.backgroundColor = KeyboardTheme.tintOTP.withAlphaComponent(0.15)
    otpBadge.textColor = KeyboardTheme.tintOTP

    if item.kind == .url, let domain = item.domain, !domain.isEmpty {
      domainLabel.isHidden = false
      domainLabel.text = domain
    } else {
      domainLabel.isHidden = true
    }

    let timeText = strings.relativeTime(fromMs: item.createdAtMs)
    metaLabel.text = "\(item.deviceName) · \(timeText)"
    pinIcon.isHidden = !item.pinned

    // Title & subtitle
    if item.kind == .otp {
      titleLabel.font = .monospacedSystemFont(ofSize: 18, weight: .bold)
      titleLabel.numberOfLines = 1
      titleLabel.text = item.formattedOTPTitle
      subtitleLabel.text = item.subtitle ?? strings.badgeOTP
      subtitleLabel.isHidden = false
    } else if item.kind == .image {
      titleLabel.font = .systemFont(ofSize: 13, weight: .medium)
      titleLabel.numberOfLines = 1
      titleLabel.text = item.title
      let meta = item.imageMeta.map { "\($0) · \(strings.imageCopyHint)" } ?? strings.imageCopyHint
      subtitleLabel.text = meta
      subtitleLabel.isHidden = false
    } else {
      titleLabel.font = .systemFont(ofSize: 13, weight: .medium)
      titleLabel.numberOfLines = 2
      titleLabel.text = item.title
      subtitleLabel.isHidden = true
    }

    // Transient green checkmark badge (completely hidden at rest)
    actedBadge.isHidden = !isActed
  }

  private func buildHierarchy() {
    contentView.backgroundColor = .clear

    cardContainer.translatesAutoresizingMaskIntoConstraints = false
    cardContainer.layer.cornerRadius = 14
    cardContainer.layer.cornerCurve = .continuous
    cardContainer.clipsToBounds = true
    contentView.addSubview(cardContainer)

    kindBar.translatesAutoresizingMaskIntoConstraints = false
    kindBar.layer.cornerRadius = 1.5

    mediaThumb.translatesAutoresizingMaskIntoConstraints = false
    mediaThumb.layer.cornerRadius = 9
    mediaThumb.layer.cornerCurve = .continuous
    mediaThumb.clipsToBounds = true
    mediaThumb.backgroundColor = KeyboardTheme.fill

    latestBadge.font = .systemFont(ofSize: 9, weight: .bold)
    otpBadge.font = .systemFont(ofSize: 9, weight: .bold)

    domainLabel.font = .systemFont(ofSize: 10, weight: .semibold)
    domainLabel.textColor = KeyboardTheme.tintURL
    domainLabel.lineBreakMode = .byTruncatingTail
    domainLabel.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)

    metaLabel.font = .systemFont(ofSize: 10, weight: .regular)
    metaLabel.textColor = .secondaryLabel
    metaLabel.lineBreakMode = .byTruncatingTail
    metaLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

    pinIcon.image = UIImage(systemName: "star.fill", withConfiguration: UIImage.SymbolConfiguration(pointSize: 9.5, weight: .semibold))
    pinIcon.tintColor = KeyboardTheme.tintPin
    pinIcon.contentMode = .scaleAspectFit

    metaStack.axis = .horizontal
    metaStack.alignment = .center
    metaStack.spacing = 5
    metaStack.addArrangedSubview(latestBadge)
    metaStack.addArrangedSubview(otpBadge)
    metaStack.addArrangedSubview(domainLabel)
    metaStack.addArrangedSubview(metaLabel)
    metaStack.addArrangedSubview(pinIcon)
    metaStack.addArrangedSubview(UIView())

    titleLabel.textColor = .label
    titleLabel.lineBreakMode = .byTruncatingTail

    subtitleLabel.font = .systemFont(ofSize: 10.5, weight: .regular)
    subtitleLabel.textColor = .tertiaryLabel
    subtitleLabel.lineBreakMode = .byTruncatingTail

    let textStack = UIStackView(arrangedSubviews: [metaStack, titleLabel, subtitleLabel])
    textStack.axis = .vertical
    textStack.spacing = 2
    textStack.translatesAutoresizingMaskIntoConstraints = false

    actedBadge.translatesAutoresizingMaskIntoConstraints = false
    actedBadge.backgroundColor = KeyboardTheme.tintGreen
    actedBadge.layer.cornerRadius = 12
    actedBadge.isHidden = true
    actedIcon.translatesAutoresizingMaskIntoConstraints = false
    actedIcon.image = UIImage(systemName: "checkmark", withConfiguration: UIImage.SymbolConfiguration(pointSize: 11, weight: .bold))
    actedIcon.tintColor = .white
    actedBadge.addSubview(actedIcon)

    let mainRow = UIStackView(arrangedSubviews: [kindBar, mediaThumb, textStack, actedBadge])
    mainRow.axis = .horizontal
    mainRow.alignment = .center
    mainRow.spacing = 9
    mainRow.translatesAutoresizingMaskIntoConstraints = false
    cardContainer.addSubview(mainRow)

    NSLayoutConstraint.activate([
      cardContainer.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
      cardContainer.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
      cardContainer.topAnchor.constraint(equalTo: contentView.topAnchor),
      cardContainer.bottomAnchor.constraint(equalTo: contentView.bottomAnchor),

      mainRow.leadingAnchor.constraint(equalTo: cardContainer.leadingAnchor, constant: 9),
      mainRow.trailingAnchor.constraint(equalTo: cardContainer.trailingAnchor, constant: -11),
      mainRow.topAnchor.constraint(equalTo: cardContainer.topAnchor, constant: 8),
      mainRow.bottomAnchor.constraint(equalTo: cardContainer.bottomAnchor, constant: -8),

      kindBar.widthAnchor.constraint(equalToConstant: 3),
      kindBar.heightAnchor.constraint(equalTo: textStack.heightAnchor, constant: -2),

      mediaThumb.widthAnchor.constraint(equalToConstant: 62),
      mediaThumb.heightAnchor.constraint(equalToConstant: 44),

      actedBadge.widthAnchor.constraint(equalToConstant: 24),
      actedBadge.heightAnchor.constraint(equalToConstant: 24),
      actedIcon.centerXAnchor.constraint(equalTo: actedBadge.centerXAnchor),
      actedIcon.centerYAnchor.constraint(equalTo: actedBadge.centerYAnchor),
    ])
  }
}

/// 2-column visual media grid tile shown when rail filter == `.image`.
@MainActor
final class KeyboardImageTileCell: UICollectionViewCell {
  static let reuseIdentifier = "KeyboardImageTileCell"

  private let container = UIView()
  private let imageView = UIImageView()
  private let gradientLayer = CAGradientLayer()
  private let badgeLabel = PaddingPillLabel()
  private let nameLabel = UILabel()
  private let metaLabel = UILabel()

  override init(frame: CGRect) {
    super.init(frame: frame)
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  override var isHighlighted: Bool {
    didSet {
      UIView.animate(withDuration: 0.1) {
        self.container.transform = self.isHighlighted ? CGAffineTransform(scaleX: 0.96, y: 0.96) : .identity
      }
    }
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    gradientLayer.frame = CGRect(x: 0, y: container.bounds.height - 46, width: container.bounds.width, height: 46)
  }

  override func prepareForReuse() {
    super.prepareForReuse()
    imageView.image = nil
  }

  func configure(item: KeyboardClipItem, isActed: Bool, strings: KeyboardStrings, thumbnail: UIImage?) {
    imageView.image = thumbnail ?? UIImage(systemName: "photo")
    imageView.contentMode = thumbnail != nil ? .scaleAspectFill : .center
    imageView.tintColor = .secondaryLabel

    badgeLabel.text = isActed ? "✓ \(strings.copiedBadge)" : strings.tapToCopyImage
    badgeLabel.backgroundColor = isActed ? KeyboardTheme.tintGreen : UIColor.black.withAlphaComponent(0.55)

    nameLabel.text = item.title
    metaLabel.text = "\(item.deviceName) · \(strings.relativeTime(fromMs: item.createdAtMs))"
  }

  private func buildHierarchy() {
    container.translatesAutoresizingMaskIntoConstraints = false
    container.layer.cornerRadius = 14
    container.layer.cornerCurve = .continuous
    container.layer.borderWidth = 0.5
    container.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    container.backgroundColor = KeyboardTheme.fill
    container.clipsToBounds = true
    contentView.addSubview(container)

    imageView.translatesAutoresizingMaskIntoConstraints = false
    imageView.clipsToBounds = true
    container.addSubview(imageView)

    gradientLayer.colors = [UIColor.clear.cgColor, UIColor.black.withAlphaComponent(0.72).cgColor]
    gradientLayer.locations = [0, 1]
    container.layer.addSublayer(gradientLayer)

    badgeLabel.translatesAutoresizingMaskIntoConstraints = false
    badgeLabel.font = .systemFont(ofSize: 9.5, weight: .semibold)
    badgeLabel.textColor = .white
    container.addSubview(badgeLabel)

    nameLabel.font = .systemFont(ofSize: 11, weight: .semibold)
    nameLabel.textColor = .white
    nameLabel.lineBreakMode = .byTruncatingTail

    metaLabel.font = .systemFont(ofSize: 9.5, weight: .regular)
    metaLabel.textColor = UIColor.white.withAlphaComponent(0.82)
    metaLabel.lineBreakMode = .byTruncatingTail

    let footStack = UIStackView(arrangedSubviews: [nameLabel, metaLabel])
    footStack.axis = .vertical
    footStack.spacing = 1
    footStack.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(footStack)

    NSLayoutConstraint.activate([
      container.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
      container.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
      container.topAnchor.constraint(equalTo: contentView.topAnchor),
      container.bottomAnchor.constraint(equalTo: contentView.bottomAnchor),

      imageView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
      imageView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
      imageView.topAnchor.constraint(equalTo: container.topAnchor),
      imageView.bottomAnchor.constraint(equalTo: container.bottomAnchor),

      badgeLabel.topAnchor.constraint(equalTo: container.topAnchor, constant: 6),
      badgeLabel.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -6),

      footStack.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 8),
      footStack.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -8),
      footStack.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -6),
    ])
  }
}

final class PaddingPillLabel: UILabel {
  private let insets = UIEdgeInsets(top: 1.5, left: 6, bottom: 1.5, right: 6)

  override init(frame: CGRect) {
    super.init(frame: frame)
    clipsToBounds = true
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  override func drawText(in rect: CGRect) {
    super.drawText(in: rect.inset(by: insets))
  }

  override var intrinsicContentSize: CGSize {
    let base = super.intrinsicContentSize
    return CGSize(width: base.width + insets.left + insets.right, height: base.height + insets.top + insets.bottom)
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    layer.cornerRadius = bounds.height / 2
  }
}
