import UIKit

/// In-keyboard Liquid Glass full-content preview sheet triggered by long-pressing any card.
@MainActor
final class KeyboardPreviewOverlayView: UIView {
  var onClose: (() -> Void)?
  var onTogglePin: ((KeyboardClipItem) -> Void)?
  var onPrimaryAction: ((KeyboardClipItem) -> Void)?

  private let sheetContainer = UIView()
  private let blurView = UIVisualEffectView(effect: UIBlurEffect(style: .systemThickMaterial))
  private let metaLabel = UILabel()
  private let pinnedBadge = UILabel()
  private let closeButton = UIButton(type: .system)
  private let textView = UITextView()
  private let imagePreviewContainer = UIView()
  private let imagePreviewView = UIImageView()
  private let imageMetaLabel = UILabel()
  private let pinButton = UIButton(type: .system)
  private let primaryButton = UIButton(type: .system)

  private var currentItem: KeyboardClipItem?

  override init(frame: CGRect) {
    super.init(frame: frame)
    isHidden = true
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  func present(item: KeyboardClipItem, image: UIImage?, strings: KeyboardStrings) {
    currentItem = item
    metaLabel.text = "\(item.deviceName) · \(strings.relativeTime(fromMs: item.createdAtMs))"
    pinnedBadge.isHidden = !item.pinned
    pinnedBadge.text = "★ \(strings.pinnedTag)"

    if item.kind == .image {
      textView.isHidden = true
      imagePreviewContainer.isHidden = false
      imagePreviewView.image = image ?? UIImage(systemName: "photo")
      imagePreviewView.contentMode = image != nil ? .scaleAspectFit : .center
      let meta = item.imageMeta.map { "\(item.title) · \($0)" } ?? item.title
      imageMetaLabel.text = meta
      primaryButton.setTitle(" \(strings.copyImage)", for: .normal)
      primaryButton.setImage(
        UIImage(systemName: "photo", withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .semibold)),
        for: .normal
      )
    } else {
      imagePreviewContainer.isHidden = true
      textView.isHidden = false
      textView.text = item.fullText ?? item.insertText ?? item.title
      textView.setContentOffset(.zero, animated: false)
      primaryButton.setTitle(" \(strings.insertIntoInput)", for: .normal)
      primaryButton.setImage(
        UIImage(systemName: "checkmark", withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .bold)),
        for: .normal
      )
    }

    pinButton.setTitle(" \(item.pinned ? strings.unpin : strings.pin)", for: .normal)
    pinButton.setImage(
      UIImage(systemName: item.pinned ? "pin.slash" : "pin", withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .semibold)),
      for: .normal
    )

    isHidden = false
    sheetContainer.alpha = 0
    sheetContainer.transform = CGAffineTransform(scaleX: 0.96, y: 0.96)
    UIView.animate(withDuration: 0.16, delay: 0, options: [.curveEaseOut]) {
      self.sheetContainer.alpha = 1
      self.sheetContainer.transform = .identity
    }
  }

  func dismiss() {
    guard !isHidden else { return }
    currentItem = nil
    UIView.animate(withDuration: 0.12, animations: {
      self.sheetContainer.alpha = 0
      self.sheetContainer.transform = CGAffineTransform(scaleX: 0.97, y: 0.97)
    }, completion: { _ in
      self.isHidden = true
      self.sheetContainer.transform = .identity
    })
  }

  private func buildHierarchy() {
    translatesAutoresizingMaskIntoConstraints = false
    backgroundColor = .clear

    sheetContainer.translatesAutoresizingMaskIntoConstraints = false
    sheetContainer.backgroundColor = KeyboardTheme.glassOverlayBackground
    sheetContainer.layer.cornerRadius = 18
    sheetContainer.layer.cornerCurve = .continuous
    sheetContainer.layer.borderWidth = 0.5
    sheetContainer.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    sheetContainer.clipsToBounds = true
    addSubview(sheetContainer)

    blurView.translatesAutoresizingMaskIntoConstraints = false
    sheetContainer.addSubview(blurView)

    metaLabel.font = .systemFont(ofSize: 11, weight: .semibold)
    metaLabel.textColor = .secondaryLabel
    metaLabel.lineBreakMode = .byTruncatingTail

    pinnedBadge.font = .systemFont(ofSize: 11, weight: .semibold)
    pinnedBadge.textColor = KeyboardTheme.tintPin

    closeButton.translatesAutoresizingMaskIntoConstraints = false
    closeButton.setImage(
      UIImage(systemName: "xmark", withConfiguration: UIImage.SymbolConfiguration(pointSize: 11, weight: .bold)),
      for: .normal
    )
    closeButton.tintColor = .label
    closeButton.backgroundColor = KeyboardTheme.fill
    closeButton.layer.cornerRadius = 13
    closeButton.addTarget(self, action: #selector(handleClose), for: .touchUpInside)

    let headerStack = UIStackView(arrangedSubviews: [metaLabel, pinnedBadge, UIView(), closeButton])
    headerStack.axis = .horizontal
    headerStack.alignment = .center
    headerStack.spacing = 6
    headerStack.translatesAutoresizingMaskIntoConstraints = false

    textView.translatesAutoresizingMaskIntoConstraints = false
    textView.isEditable = false
    textView.isSelectable = true
    textView.contentInsetAdjustmentBehavior = .never
    #if compiler(>=6.2)
    if #available(iOS 26.0, *) {
      textView.topEdgeEffect.isHidden = true
      textView.bottomEdgeEffect.isHidden = true
      textView.leftEdgeEffect.isHidden = true
      textView.rightEdgeEffect.isHidden = true
    }
    #endif
    textView.backgroundColor = KeyboardTheme.fill
    textView.layer.cornerRadius = 12
    textView.layer.cornerCurve = .continuous
    textView.textContainerInset = UIEdgeInsets(top: 10, left: 10, bottom: 10, right: 10)
    textView.font = .systemFont(ofSize: 13)
    textView.textColor = .label

    imagePreviewContainer.translatesAutoresizingMaskIntoConstraints = false
    imagePreviewView.translatesAutoresizingMaskIntoConstraints = false
    imagePreviewView.layer.cornerRadius = 12
    imagePreviewView.layer.cornerCurve = .continuous
    imagePreviewView.clipsToBounds = true
    imagePreviewView.tintColor = .secondaryLabel
    imageMetaLabel.translatesAutoresizingMaskIntoConstraints = false
    imageMetaLabel.font = .systemFont(ofSize: 11)
    imageMetaLabel.textColor = .secondaryLabel
    imageMetaLabel.textAlignment = .center
    imageMetaLabel.lineBreakMode = .byTruncatingMiddle
    imagePreviewContainer.addSubview(imagePreviewView)
    imagePreviewContainer.addSubview(imageMetaLabel)

    let contentSlot = UIView()
    contentSlot.translatesAutoresizingMaskIntoConstraints = false
    contentSlot.addSubview(textView)
    contentSlot.addSubview(imagePreviewContainer)

    pinButton.translatesAutoresizingMaskIntoConstraints = false
    pinButton.backgroundColor = KeyboardTheme.fill
    pinButton.setTitleColor(.label, for: .normal)
    pinButton.tintColor = .label
    pinButton.titleLabel?.font = .systemFont(ofSize: 12.5, weight: .semibold)
    pinButton.layer.cornerRadius = 11
    pinButton.layer.cornerCurve = .continuous
    pinButton.addTarget(self, action: #selector(handlePin), for: .touchUpInside)

    primaryButton.translatesAutoresizingMaskIntoConstraints = false
    primaryButton.backgroundColor = KeyboardTheme.accent
    primaryButton.setTitleColor(KeyboardTheme.onAccent, for: .normal)
    primaryButton.tintColor = KeyboardTheme.onAccent
    primaryButton.titleLabel?.font = .systemFont(ofSize: 12.5, weight: .semibold)
    primaryButton.layer.cornerRadius = 11
    primaryButton.layer.cornerCurve = .continuous
    primaryButton.addTarget(self, action: #selector(handlePrimary), for: .touchUpInside)

    let actionStack = UIStackView(arrangedSubviews: [pinButton, primaryButton])
    actionStack.axis = .horizontal
    actionStack.distribution = .fillEqually
    actionStack.spacing = 8
    actionStack.translatesAutoresizingMaskIntoConstraints = false

    let root = UIStackView(arrangedSubviews: [headerStack, contentSlot, actionStack])
    root.axis = .vertical
    root.spacing = 10
    root.translatesAutoresizingMaskIntoConstraints = false
    sheetContainer.addSubview(root)

    NSLayoutConstraint.activate([
      sheetContainer.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
      sheetContainer.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
      sheetContainer.topAnchor.constraint(equalTo: topAnchor, constant: 6),
      sheetContainer.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -6),

      blurView.leadingAnchor.constraint(equalTo: sheetContainer.leadingAnchor),
      blurView.trailingAnchor.constraint(equalTo: sheetContainer.trailingAnchor),
      blurView.topAnchor.constraint(equalTo: sheetContainer.topAnchor),
      blurView.bottomAnchor.constraint(equalTo: sheetContainer.bottomAnchor),

      root.leadingAnchor.constraint(equalTo: sheetContainer.leadingAnchor, constant: 14),
      root.trailingAnchor.constraint(equalTo: sheetContainer.trailingAnchor, constant: -14),
      root.topAnchor.constraint(equalTo: sheetContainer.topAnchor, constant: 12),
      root.bottomAnchor.constraint(equalTo: sheetContainer.bottomAnchor, constant: -12),

      closeButton.widthAnchor.constraint(equalToConstant: 26),
      closeButton.heightAnchor.constraint(equalToConstant: 26),

      textView.leadingAnchor.constraint(equalTo: contentSlot.leadingAnchor),
      textView.trailingAnchor.constraint(equalTo: contentSlot.trailingAnchor),
      textView.topAnchor.constraint(equalTo: contentSlot.topAnchor),
      textView.bottomAnchor.constraint(equalTo: contentSlot.bottomAnchor),

      imagePreviewContainer.leadingAnchor.constraint(equalTo: contentSlot.leadingAnchor),
      imagePreviewContainer.trailingAnchor.constraint(equalTo: contentSlot.trailingAnchor),
      imagePreviewContainer.topAnchor.constraint(equalTo: contentSlot.topAnchor),
      imagePreviewContainer.bottomAnchor.constraint(equalTo: contentSlot.bottomAnchor),

      imagePreviewView.leadingAnchor.constraint(equalTo: imagePreviewContainer.leadingAnchor),
      imagePreviewView.trailingAnchor.constraint(equalTo: imagePreviewContainer.trailingAnchor),
      imagePreviewView.topAnchor.constraint(equalTo: imagePreviewContainer.topAnchor),
      imagePreviewView.bottomAnchor.constraint(equalTo: imageMetaLabel.topAnchor, constant: -6),

      imageMetaLabel.leadingAnchor.constraint(equalTo: imagePreviewContainer.leadingAnchor),
      imageMetaLabel.trailingAnchor.constraint(equalTo: imagePreviewContainer.trailingAnchor),
      imageMetaLabel.bottomAnchor.constraint(equalTo: imagePreviewContainer.bottomAnchor),

      actionStack.heightAnchor.constraint(equalToConstant: 36),
    ])
  }

  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    sheetContainer.layer.borderColor = KeyboardTheme.cardBorder.cgColor
  }

  @objc private func handleClose() { onClose?() }
  @objc private func handlePin() {
    guard let item = currentItem else { return }
    onTogglePin?(item)
  }
  @objc private func handlePrimary() {
    guard let item = currentItem else { return }
    onPrimaryAction?(item)
  }
}
