import UIKit

@MainActor
final class KeyboardEditBarView: UIView {
  var onUndo: (() -> Void)?
  var onSpace: (() -> Void)?
  var onScrubCursor: ((Int) -> Void)?
  var onReturn: (() -> Void)?
  var onDeleteBackward: ((Bool) -> Void)?
  var onAdvanceInputMode: (() -> Void)?

  private let rootStack = UIStackView()
  private let editRow = UIStackView()
  private let undoButton = KeycapButton()
  private let spaceButton = KeycapButton()
  private let returnButton = KeycapButton()
  private let deleteButton = RepeatKeycapButton()
  private let globeBand = UIView()
  private let globeButton = UIButton(type: .system)

  private var deleteRepeatTask: Task<Void, Never>?
  private var isScrubbing = false
  private var lastScrubX: CGFloat = 0
  private var currentStrings = KeyboardStrings()

  override init(frame: CGRect) {
    super.init(frame: frame)
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  deinit {
    deleteRepeatTask?.cancel()
  }

  func configure(
    canUndo: Bool,
    returnTitle: String,
    showsEditRow: Bool,
    showsGlobeBand: Bool,
    strings: KeyboardStrings
  ) {
    currentStrings = strings
    editRow.isHidden = !showsEditRow
    globeBand.isHidden = !showsGlobeBand

    undoButton.setTitle(" \(strings.undo)", for: .normal)
    undoButton.setImage(
      UIImage(systemName: "arrow.uturn.backward", withConfiguration: UIImage.SymbolConfiguration(pointSize: 12, weight: .semibold)),
      for: .normal
    )
    if canUndo {
      undoButton.style = .undoReady
    } else {
      undoButton.style = .mutedDisabled
    }

    if !isScrubbing {
      spaceButton.setTitle(strings.space, for: .normal)
      spaceButton.style = .standard
    }

    returnButton.setTitle(returnTitle, for: .normal)
    returnButton.style = .ink

    deleteButton.accessibilityLabel = strings.deleteLabel
    globeButton.accessibilityLabel = strings.globeLabel
  }

  private func buildHierarchy() {
    translatesAutoresizingMaskIntoConstraints = false

    rootStack.axis = .vertical
    rootStack.spacing = 4
    rootStack.translatesAutoresizingMaskIntoConstraints = false
    addSubview(rootStack)

    editRow.axis = .horizontal
    editRow.spacing = 6
    editRow.translatesAutoresizingMaskIntoConstraints = false

    undoButton.style = .mutedDisabled
    undoButton.titleLabel?.font = .systemFont(ofSize: 12.5, weight: .semibold)
    undoButton.addTarget(self, action: #selector(handleUndo), for: .touchUpInside)

    spaceButton.style = .standard
    spaceButton.titleLabel?.font = .systemFont(ofSize: 13, weight: .medium)
    spaceButton.addTarget(self, action: #selector(handleSpace), for: .touchUpInside)

    let scrubPan = UILongPressGestureRecognizer(target: self, action: #selector(handleSpaceScrub(_:)))
    scrubPan.minimumPressDuration = 0.28
    scrubPan.allowableMovement = 400
    spaceButton.addGestureRecognizer(scrubPan)

    returnButton.style = .ink
    returnButton.titleLabel?.font = .systemFont(ofSize: 13, weight: .semibold)
    returnButton.addTarget(self, action: #selector(handleReturn), for: .touchUpInside)

    deleteButton.style = .muted
    deleteButton.setImage(
      UIImage(systemName: "delete.left", withConfiguration: UIImage.SymbolConfiguration(pointSize: 15, weight: .medium)),
      for: .normal
    )
    deleteButton.onTouchDown = { [weak self] in self?.startDeleting() }
    deleteButton.onTouchUp = { [weak self] in self?.stopDeleting() }

    editRow.addArrangedSubview(undoButton)
    editRow.addArrangedSubview(spaceButton)
    editRow.addArrangedSubview(returnButton)
    editRow.addArrangedSubview(deleteButton)

    globeBand.translatesAutoresizingMaskIntoConstraints = false
    globeButton.translatesAutoresizingMaskIntoConstraints = false
    globeButton.setImage(
      UIImage(systemName: "globe", withConfiguration: UIImage.SymbolConfiguration(pointSize: 17, weight: .medium)),
      for: .normal
    )
    globeButton.tintColor = .secondaryLabel
    globeButton.addTarget(self, action: #selector(handleGlobe), for: .touchUpInside)
    globeBand.addSubview(globeButton)

    rootStack.addArrangedSubview(editRow)
    rootStack.addArrangedSubview(globeBand)

    NSLayoutConstraint.activate([
      rootStack.leadingAnchor.constraint(equalTo: leadingAnchor),
      rootStack.trailingAnchor.constraint(equalTo: trailingAnchor),
      rootStack.topAnchor.constraint(equalTo: topAnchor),
      rootStack.bottomAnchor.constraint(equalTo: bottomAnchor),

      editRow.heightAnchor.constraint(equalToConstant: KeyboardLayoutMetrics.editBarHeight),
      undoButton.widthAnchor.constraint(equalToConstant: 68),
      returnButton.widthAnchor.constraint(equalToConstant: 72),
      deleteButton.widthAnchor.constraint(equalToConstant: 54),

      globeBand.heightAnchor.constraint(equalToConstant: KeyboardLayoutMetrics.globeStripHeight),
      globeButton.leadingAnchor.constraint(equalTo: globeBand.leadingAnchor, constant: 4),
      globeButton.centerYAnchor.constraint(equalTo: globeBand.centerYAnchor),
      globeButton.widthAnchor.constraint(equalToConstant: 34),
      globeButton.heightAnchor.constraint(equalToConstant: 30),
    ])
  }

  @objc private func handleUndo() { onUndo?() }
  @objc private func handleSpace() {
    guard !isScrubbing else { return }
    onSpace?()
  }
  @objc private func handleReturn() { onReturn?() }
  @objc private func handleGlobe() { onAdvanceInputMode?() }

  @objc private func handleSpaceScrub(_ gesture: UILongPressGestureRecognizer) {
    let x = gesture.location(in: self).x
    switch gesture.state {
    case .began:
      isScrubbing = true
      lastScrubX = x
      spaceButton.style = .ink
      spaceButton.setTitle(currentStrings.spaceScrubbing, for: .normal)
    case .changed:
      let delta = x - lastScrubX
      let stepThreshold: CGFloat = 10
      let steps = Int(delta / stepThreshold)
      if steps != 0 {
        lastScrubX += CGFloat(steps) * stepThreshold
        onScrubCursor?(steps)
      }
    case .ended, .cancelled, .failed:
      isScrubbing = false
      spaceButton.style = .standard
      spaceButton.setTitle(currentStrings.space, for: .normal)
    default:
      break
    }
  }

  private func startDeleting() {
    deleteRepeatTask?.cancel()
    deleteRepeatTask = Task { @MainActor [weak self] in
      guard let self else { return }
      self.onDeleteBackward?(false)
      try? await Task.sleep(for: .seconds(0.40))
      var interval = 0.095
      var count = 0
      while !Task.isCancelled, count < 600 {
        self.onDeleteBackward?(true)
        count += 1
        try? await Task.sleep(for: .seconds(interval))
        interval = max(0.035, interval * 0.90)
      }
    }
  }

  private func stopDeleting() {
    deleteRepeatTask?.cancel()
    deleteRepeatTask = nil
  }
}

@MainActor
private class KeycapButton: UIButton {
  enum Style {
    case standard
    case muted
    case mutedDisabled
    case undoReady
    case ink
  }

  var style: Style = .standard {
    didSet { applyStyle() }
  }

  override init(frame: CGRect) {
    super.init(frame: frame)
    translatesAutoresizingMaskIntoConstraints = false
    layer.cornerRadius = 11
    layer.cornerCurve = .continuous
    applyStyle()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  override var isHighlighted: Bool {
    didSet {
      UIView.animate(withDuration: 0.08) {
        self.transform = self.isHighlighted ? CGAffineTransform(scaleX: 0.96, y: 0.96) : .identity
        self.alpha = self.isHighlighted && self.style == .ink ? 0.86 : (self.style == .mutedDisabled ? 0.48 : 1.0)
      }
    }
  }

  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    applyStyle()
  }

  private func applyStyle() {
    layer.borderWidth = 0
    layer.borderColor = nil
    alpha = 1.0
    switch style {
    case .standard:
      backgroundColor = KeyboardTheme.keyBackground
      setTitleColor(.label, for: .normal)
      tintColor = .label
    case .muted:
      backgroundColor = KeyboardTheme.keyMutedBackground
      setTitleColor(.label, for: .normal)
      tintColor = .label
    case .mutedDisabled:
      backgroundColor = KeyboardTheme.keyMutedBackground
      setTitleColor(.secondaryLabel, for: .normal)
      tintColor = .secondaryLabel
      alpha = 0.48
    case .undoReady:
      backgroundColor = KeyboardTheme.undoReadyBackground
      setTitleColor(KeyboardTheme.tintAmber, for: .normal)
      tintColor = KeyboardTheme.tintAmber
      layer.borderWidth = 0.5
      layer.borderColor = KeyboardTheme.undoReadyBorder.cgColor
    case .ink:
      backgroundColor = KeyboardTheme.accent
      setTitleColor(KeyboardTheme.onAccent, for: .normal)
      tintColor = KeyboardTheme.onAccent
    }
  }
}

@MainActor
private final class RepeatKeycapButton: KeycapButton {
  var onTouchDown: (() -> Void)?
  var onTouchUp: (() -> Void)?

  override init(frame: CGRect) {
    super.init(frame: frame)
    addTarget(self, action: #selector(handleDown), for: .touchDown)
    addTarget(self, action: #selector(handleUp), for: [.touchUpInside, .touchUpOutside, .touchCancel, .touchDragExit])
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  @objc private func handleDown() { onTouchDown?() }
  @objc private func handleUp() { onTouchUp?() }
}
