import UIKit

enum KeyboardSendDeviceState: Equatable {
  case waiting
  case sending(progressPercent: Int?)
  case delivered
  case offline
  case failed

  var isActive: Bool {
    switch self {
    case .waiting, .sending: return true
    case .delivered, .offline, .failed: return false
    }
  }
}

struct KeyboardSendDeviceStatus: Equatable, Identifiable {
  let id: String
  let name: String
  let state: KeyboardSendDeviceState
}

enum KeyboardSendOverallPhase: Equatable {
  case connecting
  case sending
  case delivered
  case partial
}

struct KeyboardSendBatchState: Equatable {
  let historyID: String
  let clipTitle: String
  let devices: [KeyboardSendDeviceStatus]
  let lastHighlightText: String?
  let isConnectingOnly: Bool

  var totalCount: Int { devices.count }
  var deliveredCount: Int { devices.filter { $0.state == .delivered }.count }
  var failedCount: Int { devices.filter { $0.state == .failed }.count }
  var offlineCount: Int { devices.filter { $0.state == .offline }.count }
  var activeCount: Int { devices.filter { $0.state.isActive }.count }

  var overall: KeyboardSendOverallPhase {
    if isConnectingOnly || (totalCount > 0 && devices.allSatisfy { $0.state == .waiting }) {
      return .connecting
    }
    if activeCount > 0 {
      return .sending
    }
    if totalCount > 0 && deliveredCount == totalCount {
      return .delivered
    }
    return .partial
  }
}

/// Scheme A: Coalesced Dynamic Pill anchored above the bottom edit bar.
/// Aggregates multi-device send progress into a single in-place morphing pill
/// with an expandable per-device status popover and single-device retry.
@MainActor
final class KeyboardSendStatusPillView: UIView {
  var onRetryDevice: ((String, String) -> Void)?

  private(set) var isExpanded = false
  private(set) var isShowingBatch = false
  private var currentBatch: KeyboardSendBatchState?
  private var currentStrings = KeyboardStrings()
  private var autoHideTask: Task<Void, Never>?
  private var temporarilySuppressed = false

  // Popover (above pill)
  private let popoverContainer = UIView()
  private let popoverBlur = UIVisualEffectView(effect: UIBlurEffect(style: .systemChromeMaterial))
  private let popoverHeaderTitle = UILabel()
  private let popoverCloseButton = UIButton(type: .system)
  private let popoverScroll = UIScrollView()
  private let popoverRowsStack = UIStackView()
  private var popoverScrollHeightConstraint: NSLayoutConstraint?

  // Floating pill
  private let pillControl = UIControl()
  private let pillStack = UIStackView()
  private let spinner = UIActivityIndicatorView(style: .medium)
  private let statusIcon = UIImageView()
  private let titleLabel = UILabel()
  private let subLabel = UILabel()
  private let dotsContainer = UIView()
  private let dotsStack = UIStackView()
  private let chevronIcon = UIImageView()

  override init(frame: CGRect) {
    super.init(frame: frame)
    translatesAutoresizingMaskIntoConstraints = false
    isHidden = true
    buildHierarchy()
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  /// Pass through all touches outside the pill and visible popover so underlying clip cards remain tappable.
  override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
    guard !isHidden, alpha > 0.01 else { return nil }
    if !popoverContainer.isHidden {
      let popPoint = convert(point, to: popoverContainer)
      if popoverContainer.bounds.contains(popPoint) {
        return popoverContainer.hitTest(popPoint, with: event)
      }
    }
    if !pillControl.isHidden {
      let pillPoint = convert(point, to: pillControl)
      if pillControl.bounds.contains(pillPoint) {
        return pillControl.hitTest(pillPoint, with: event)
      }
    }
    return nil
  }

  func update(batch: KeyboardSendBatchState, strings: KeyboardStrings) {
    currentStrings = strings
    let wasVisible = isShowingBatch && !isHidden
    let isNewHistory = currentBatch?.historyID != batch.historyID
    currentBatch = batch
    isShowingBatch = true

    if isNewHistory && isExpanded {
      setExpanded(false, animated: false)
    }

    renderPill(batch: batch, strings: strings)
    if isExpanded {
      renderPopoverRows(batch: batch, strings: strings)
    }

    if !wasVisible {
      isHidden = false
      if !temporarilySuppressed {
        pillControl.alpha = 0
        pillControl.transform = CGAffineTransform(translationX: 0, y: 6).scaledBy(x: 0.95, y: 0.95)
        UIView.animate(withDuration: 0.2, delay: 0, usingSpringWithDamping: 0.86, initialSpringVelocity: 0.4) {
          self.pillControl.alpha = 1
          self.pillControl.transform = .identity
        }
      }
    } else if !temporarilySuppressed {
      UIView.animate(withDuration: 0.16) {
        self.pillControl.alpha = 1
        self.pillControl.transform = .identity
        self.layoutIfNeeded()
      }
    }

    scheduleAutoHideIfNeeded(for: batch)
  }

  func dismiss(animated: Bool = true) {
    autoHideTask?.cancel()
    autoHideTask = nil
    isShowingBatch = false
    currentBatch = nil
    setExpanded(false, animated: false)
    guard !isHidden else { return }
    if animated {
      UIView.animate(withDuration: 0.18, animations: {
        self.pillControl.alpha = 0
        self.pillControl.transform = CGAffineTransform(translationX: 0, y: 5)
      }, completion: { _ in
        if !self.isShowingBatch {
          self.isHidden = true
          self.pillControl.transform = .identity
        }
      })
    } else {
      isHidden = true
      pillControl.alpha = 0
      pillControl.transform = .identity
    }
  }

  func collapse() {
    guard isExpanded else { return }
    setExpanded(false, animated: true)
    if let batch = currentBatch {
      scheduleAutoHideIfNeeded(for: batch)
    }
  }

  /// Temporarily fades the pill while an ordinary one-off toast (e.g. Inserted / Received) is shown
  /// so at most one capsule ever appears above the edit bar.
  func setTemporarilySuppressed(_ suppressed: Bool) {
    temporarilySuppressed = suppressed
    guard isShowingBatch, !isHidden else { return }
    if suppressed && isExpanded {
      setExpanded(false, animated: false)
    }
    UIView.animate(withDuration: 0.14) {
      self.pillControl.alpha = suppressed ? 0 : 1
    }
  }

  // MARK: - Hierarchy

  private func buildHierarchy() {
    // Popover card (above pill)
    popoverContainer.translatesAutoresizingMaskIntoConstraints = false
    popoverContainer.backgroundColor = KeyboardTheme.glassOverlayBackground
    popoverContainer.layer.cornerRadius = 15
    popoverContainer.layer.cornerCurve = .continuous
    popoverContainer.layer.borderWidth = 0.5
    popoverContainer.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    popoverContainer.layer.shadowColor = UIColor.black.cgColor
    popoverContainer.layer.shadowOpacity = 0.18
    popoverContainer.layer.shadowRadius = 14
    popoverContainer.layer.shadowOffset = CGSize(width: 0, height: 6)
    popoverContainer.isHidden = true
    addSubview(popoverContainer)

    let popoverClip = UIView()
    popoverClip.translatesAutoresizingMaskIntoConstraints = false
    popoverClip.layer.cornerRadius = 15
    popoverClip.layer.cornerCurve = .continuous
    popoverClip.clipsToBounds = true
    popoverContainer.addSubview(popoverClip)

    popoverBlur.translatesAutoresizingMaskIntoConstraints = false
    popoverClip.addSubview(popoverBlur)

    popoverHeaderTitle.font = .systemFont(ofSize: 11.5, weight: .bold)
    popoverHeaderTitle.textColor = .label
    popoverHeaderTitle.lineBreakMode = .byTruncatingTail
    popoverHeaderTitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

    var closeConfig = UIButton.Configuration.filled()
    closeConfig.baseBackgroundColor = KeyboardTheme.fill
    closeConfig.baseForegroundColor = .secondaryLabel
    closeConfig.cornerStyle = .small
    closeConfig.contentInsets = NSDirectionalEdgeInsets(top: 2.5, leading: 7, bottom: 2.5, trailing: 7)
    closeConfig.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
      var out = incoming
      out.font = .systemFont(ofSize: 10.5, weight: .semibold)
      return out
    }
    popoverCloseButton.configuration = closeConfig
    popoverCloseButton.addTarget(self, action: #selector(handleCollapseTap), for: .touchUpInside)
    popoverCloseButton.setContentCompressionResistancePriority(.required, for: .horizontal)

    let headerRow = UIStackView(arrangedSubviews: [popoverHeaderTitle, popoverCloseButton])
    headerRow.axis = .horizontal
    headerRow.alignment = .center
    headerRow.spacing = 8
    headerRow.translatesAutoresizingMaskIntoConstraints = false

    let divider = UIView()
    divider.translatesAutoresizingMaskIntoConstraints = false
    divider.backgroundColor = KeyboardTheme.separator

    popoverScroll.translatesAutoresizingMaskIntoConstraints = false
    popoverScroll.showsVerticalScrollIndicator = false
    popoverScroll.alwaysBounceVertical = false
    popoverScroll.contentInsetAdjustmentBehavior = .never
    #if compiler(>=6.2)
    if #available(iOS 26.0, *) {
      popoverScroll.topEdgeEffect.isHidden = true
      popoverScroll.bottomEdgeEffect.isHidden = true
      popoverScroll.leftEdgeEffect.isHidden = true
      popoverScroll.rightEdgeEffect.isHidden = true
    }
    #endif

    popoverRowsStack.axis = .vertical
    popoverRowsStack.spacing = 7
    popoverRowsStack.translatesAutoresizingMaskIntoConstraints = false
    popoverScroll.addSubview(popoverRowsStack)

    popoverClip.addSubview(headerRow)
    popoverClip.addSubview(divider)
    popoverClip.addSubview(popoverScroll)

    // Floating Coalesced Pill
    pillControl.translatesAutoresizingMaskIntoConstraints = false
    pillControl.backgroundColor = KeyboardTheme.accent
    pillControl.layer.cornerRadius = 15
    pillControl.layer.cornerCurve = .continuous
    pillControl.layer.shadowColor = UIColor.black.cgColor
    pillControl.layer.shadowOpacity = 0.22
    pillControl.layer.shadowRadius = 10
    pillControl.layer.shadowOffset = CGSize(width: 0, height: 4)
    pillControl.addTarget(self, action: #selector(handlePillTap), for: .touchUpInside)
    addSubview(pillControl)

    spinner.translatesAutoresizingMaskIntoConstraints = false
    spinner.color = KeyboardTheme.onAccent
    spinner.transform = CGAffineTransform(scaleX: 0.68, y: 0.68)
    spinner.hidesWhenStopped = true

    statusIcon.translatesAutoresizingMaskIntoConstraints = false
    statusIcon.contentMode = .scaleAspectFit
    statusIcon.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 12, weight: .bold)

    titleLabel.font = .systemFont(ofSize: 11.5, weight: .semibold)
    titleLabel.textColor = KeyboardTheme.onAccent
    titleLabel.lineBreakMode = .byTruncatingTail
    titleLabel.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)

    subLabel.font = .systemFont(ofSize: 10.5, weight: .medium)
    subLabel.textColor = KeyboardTheme.onAccent.withAlphaComponent(0.68)
    subLabel.lineBreakMode = .byTruncatingTail
    subLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

    dotsContainer.translatesAutoresizingMaskIntoConstraints = false
    dotsContainer.backgroundColor = KeyboardTheme.onAccent.withAlphaComponent(0.14)
    dotsContainer.layer.cornerRadius = 6
    dotsStack.axis = .horizontal
    dotsStack.alignment = .center
    dotsStack.spacing = 3.5
    dotsStack.translatesAutoresizingMaskIntoConstraints = false
    dotsContainer.addSubview(dotsStack)

    chevronIcon.translatesAutoresizingMaskIntoConstraints = false
    chevronIcon.image = UIImage(systemName: "chevron.up", withConfiguration: UIImage.SymbolConfiguration(pointSize: 9.5, weight: .bold))
    chevronIcon.tintColor = KeyboardTheme.onAccent.withAlphaComponent(0.66)
    chevronIcon.contentMode = .scaleAspectFit

    pillStack.axis = .horizontal
    pillStack.alignment = .center
    pillStack.spacing = 6
    pillStack.isUserInteractionEnabled = false
    pillStack.translatesAutoresizingMaskIntoConstraints = false
    pillStack.addArrangedSubview(spinner)
    pillStack.addArrangedSubview(statusIcon)
    pillStack.addArrangedSubview(titleLabel)
    pillStack.addArrangedSubview(subLabel)
    pillStack.addArrangedSubview(dotsContainer)
    pillStack.addArrangedSubview(chevronIcon)
    pillControl.addSubview(pillStack)

    let scrollHeight = popoverScroll.heightAnchor.constraint(equalToConstant: 24)
    scrollHeight.priority = .defaultHigh
    popoverScrollHeightConstraint = scrollHeight

    NSLayoutConstraint.activate([
      // Pill anchored centered at bottom of wrapper
      pillControl.bottomAnchor.constraint(equalTo: bottomAnchor),
      pillControl.centerXAnchor.constraint(equalTo: centerXAnchor),
      pillControl.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor),
      pillControl.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
      pillControl.heightAnchor.constraint(equalToConstant: 30),

      pillStack.leadingAnchor.constraint(equalTo: pillControl.leadingAnchor, constant: 10),
      pillStack.trailingAnchor.constraint(equalTo: pillControl.trailingAnchor, constant: -10),
      pillStack.centerYAnchor.constraint(equalTo: pillControl.centerYAnchor),

      spinner.widthAnchor.constraint(equalToConstant: 14),
      spinner.heightAnchor.constraint(equalToConstant: 14),
      statusIcon.widthAnchor.constraint(equalToConstant: 14),
      statusIcon.heightAnchor.constraint(equalToConstant: 14),

      dotsStack.leadingAnchor.constraint(equalTo: dotsContainer.leadingAnchor, constant: 5),
      dotsStack.trailingAnchor.constraint(equalTo: dotsContainer.trailingAnchor, constant: -5),
      dotsStack.topAnchor.constraint(equalTo: dotsContainer.topAnchor, constant: 3.5),
      dotsStack.bottomAnchor.constraint(equalTo: dotsContainer.bottomAnchor, constant: -3.5),

      chevronIcon.widthAnchor.constraint(equalToConstant: 10),
      chevronIcon.heightAnchor.constraint(equalToConstant: 10),

      // Popover anchored 6pt above pill
      popoverContainer.bottomAnchor.constraint(equalTo: pillControl.topAnchor, constant: -6),
      popoverContainer.centerXAnchor.constraint(equalTo: centerXAnchor),
      popoverContainer.widthAnchor.constraint(lessThanOrEqualToConstant: 296),
      popoverContainer.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor),
      popoverContainer.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
      popoverContainer.topAnchor.constraint(greaterThanOrEqualTo: topAnchor),

      popoverClip.leadingAnchor.constraint(equalTo: popoverContainer.leadingAnchor),
      popoverClip.trailingAnchor.constraint(equalTo: popoverContainer.trailingAnchor),
      popoverClip.topAnchor.constraint(equalTo: popoverContainer.topAnchor),
      popoverClip.bottomAnchor.constraint(equalTo: popoverContainer.bottomAnchor),

      popoverBlur.leadingAnchor.constraint(equalTo: popoverClip.leadingAnchor),
      popoverBlur.trailingAnchor.constraint(equalTo: popoverClip.trailingAnchor),
      popoverBlur.topAnchor.constraint(equalTo: popoverClip.topAnchor),
      popoverBlur.bottomAnchor.constraint(equalTo: popoverClip.bottomAnchor),

      headerRow.leadingAnchor.constraint(equalTo: popoverClip.leadingAnchor, constant: 11),
      headerRow.trailingAnchor.constraint(equalTo: popoverClip.trailingAnchor, constant: -10),
      headerRow.topAnchor.constraint(equalTo: popoverClip.topAnchor, constant: 9),

      divider.leadingAnchor.constraint(equalTo: popoverClip.leadingAnchor, constant: 11),
      divider.trailingAnchor.constraint(equalTo: popoverClip.trailingAnchor, constant: -11),
      divider.topAnchor.constraint(equalTo: headerRow.bottomAnchor, constant: 6),
      divider.heightAnchor.constraint(equalToConstant: 0.5),

      popoverScroll.leadingAnchor.constraint(equalTo: popoverClip.leadingAnchor, constant: 11),
      popoverScroll.trailingAnchor.constraint(equalTo: popoverClip.trailingAnchor, constant: -11),
      popoverScroll.topAnchor.constraint(equalTo: divider.bottomAnchor, constant: 7),
      popoverScroll.bottomAnchor.constraint(equalTo: popoverClip.bottomAnchor, constant: -9),
      popoverScroll.widthAnchor.constraint(greaterThanOrEqualToConstant: 248),
      scrollHeight,

      popoverRowsStack.leadingAnchor.constraint(equalTo: popoverScroll.contentLayoutGuide.leadingAnchor),
      popoverRowsStack.trailingAnchor.constraint(equalTo: popoverScroll.contentLayoutGuide.trailingAnchor),
      popoverRowsStack.topAnchor.constraint(equalTo: popoverScroll.contentLayoutGuide.topAnchor),
      popoverRowsStack.bottomAnchor.constraint(equalTo: popoverScroll.contentLayoutGuide.bottomAnchor),
      popoverRowsStack.widthAnchor.constraint(equalTo: popoverScroll.frameLayoutGuide.widthAnchor),
    ])
  }

  // MARK: - Rendering

  private func renderPill(batch: KeyboardSendBatchState, strings: KeyboardStrings) {
    let overall = batch.overall
    switch overall {
    case .connecting, .sending:
      statusIcon.isHidden = true
      spinner.isHidden = false
      if !spinner.isAnimating { spinner.startAnimating() }
    case .delivered:
      spinner.stopAnimating()
      spinner.isHidden = true
      statusIcon.isHidden = false
      statusIcon.image = UIImage(systemName: "checkmark.circle.fill")
      statusIcon.tintColor = KeyboardTheme.tintGreen
    case .partial:
      spinner.stopAnimating()
      spinner.isHidden = true
      statusIcon.isHidden = false
      statusIcon.image = UIImage(systemName: "exclamationmark.circle.fill")
      statusIcon.tintColor = KeyboardTheme.tintAmber
    }

    titleLabel.text = strings.sendPillHeadline(for: batch)

    if overall == .sending, batch.totalCount > 1, let hl = batch.lastHighlightText, !hl.isEmpty {
      subLabel.text = "· \(hl)"
      subLabel.isHidden = false
    } else {
      subLabel.text = nil
      subLabel.isHidden = true
    }

    // Multi-device status dot cluster (only when >1 target devices)
    if batch.totalCount > 1 {
      dotsContainer.isHidden = false
      for v in dotsStack.arrangedSubviews {
        dotsStack.removeArrangedSubview(v)
        v.removeFromSuperview()
      }
      for dev in batch.devices.prefix(6) {
        let dot = UIView()
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.layer.cornerRadius = 2.75
        dot.backgroundColor = dotColor(for: dev.state)
        NSLayoutConstraint.activate([
          dot.widthAnchor.constraint(equalToConstant: 5.5),
          dot.heightAnchor.constraint(equalToConstant: 5.5),
        ])
        dotsStack.addArrangedSubview(dot)
      }
    } else {
      dotsContainer.isHidden = true
    }

    let canExpand = !batch.devices.isEmpty
    chevronIcon.isHidden = !canExpand
    pillControl.isUserInteractionEnabled = canExpand
  }

  private func renderPopoverRows(batch: KeyboardSendBatchState, strings: KeyboardStrings) {
    popoverHeaderTitle.text = strings.sendDetailHeader(clipTitle: batch.clipTitle)
    var closeCfg = popoverCloseButton.configuration ?? .filled()
    closeCfg.title = strings.sendDetailClose
    popoverCloseButton.configuration = closeCfg

    for v in popoverRowsStack.arrangedSubviews {
      popoverRowsStack.removeArrangedSubview(v)
      v.removeFromSuperview()
    }

    for dev in batch.devices {
      let row = makeDeviceRow(historyID: batch.historyID, device: dev, strings: strings)
      popoverRowsStack.addArrangedSubview(row)
    }

    let rowCount = max(1, batch.devices.count)
    let idealHeight = CGFloat(rowCount) * 24 + CGFloat(max(0, rowCount - 1)) * 7
    popoverScrollHeightConstraint?.constant = min(132, idealHeight)
  }

  private func makeDeviceRow(
    historyID: String,
    device: KeyboardSendDeviceStatus,
    strings: KeyboardStrings
  ) -> UIView {
    let iconBadge = UIView()
    iconBadge.translatesAutoresizingMaskIntoConstraints = false
    iconBadge.backgroundColor = KeyboardTheme.fill
    iconBadge.layer.cornerRadius = 5

    let iconImg = UIImageView(
      image: UIImage(
        systemName: symbolName(for: device.name),
        withConfiguration: UIImage.SymbolConfiguration(pointSize: 10, weight: .semibold)
      )
    )
    iconImg.translatesAutoresizingMaskIntoConstraints = false
    iconImg.tintColor = .secondaryLabel
    iconImg.contentMode = .scaleAspectFit
    iconBadge.addSubview(iconImg)

    let nameLabel = UILabel()
    nameLabel.font = .systemFont(ofSize: 11.5, weight: .semibold)
    nameLabel.textColor = .label
    nameLabel.lineBreakMode = .byTruncatingTail
    nameLabel.text = device.name
    nameLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

    let stateLabel = UILabel()
    stateLabel.font = .systemFont(ofSize: 10.5, weight: .semibold)
    stateLabel.textColor = stateTextColor(for: device.state)
    stateLabel.text = strings.sendDeviceStateLabel(device.state)
    stateLabel.setContentCompressionResistancePriority(.required, for: .horizontal)

    let spacer = UIView()
    spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)

    let row = UIStackView(arrangedSubviews: [iconBadge, nameLabel, spacer, stateLabel])
    row.axis = .horizontal
    row.alignment = .center
    row.spacing = 6

    if device.state == .failed || device.state == .offline {
      var retryConfig = UIButton.Configuration.filled()
      retryConfig.title = strings.sendRetrySingle
      retryConfig.baseBackgroundColor = KeyboardTheme.accent
      retryConfig.baseForegroundColor = KeyboardTheme.onAccent
      retryConfig.cornerStyle = .small
      retryConfig.contentInsets = NSDirectionalEdgeInsets(top: 2.5, leading: 7, bottom: 2.5, trailing: 7)
      retryConfig.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
        var out = incoming
        out.font = .systemFont(ofSize: 10, weight: .semibold)
        return out
      }
      let retryBtn = UIButton(configuration: retryConfig)
      retryBtn.setContentCompressionResistancePriority(.required, for: .horizontal)
      let devID = device.id
      retryBtn.addAction(UIAction { [weak self] _ in
        self?.onRetryDevice?(historyID, devID)
      }, for: .touchUpInside)
      row.addArrangedSubview(retryBtn)
    }

    NSLayoutConstraint.activate([
      iconBadge.widthAnchor.constraint(equalToConstant: 20),
      iconBadge.heightAnchor.constraint(equalToConstant: 20),
      iconImg.centerXAnchor.constraint(equalTo: iconBadge.centerXAnchor),
      iconImg.centerYAnchor.constraint(equalTo: iconBadge.centerYAnchor),
      row.heightAnchor.constraint(greaterThanOrEqualToConstant: 24),
    ])
    return row
  }

  private func setExpanded(_ expand: Bool, animated: Bool) {
    guard isExpanded != expand else { return }
    isExpanded = expand
    if expand {
      autoHideTask?.cancel()
      autoHideTask = nil
      if let batch = currentBatch {
        renderPopoverRows(batch: batch, strings: currentStrings)
      }
      popoverContainer.isHidden = false
      if animated {
        popoverContainer.alpha = 0
        popoverContainer.transform = CGAffineTransform(translationX: 0, y: 6).scaledBy(x: 0.96, y: 0.96)
        UIView.animate(withDuration: 0.2, delay: 0, usingSpringWithDamping: 0.88, initialSpringVelocity: 0.3) {
          self.popoverContainer.alpha = 1
          self.popoverContainer.transform = .identity
          self.chevronIcon.transform = CGAffineTransform(rotationAngle: .pi)
        }
      } else {
        popoverContainer.alpha = 1
        popoverContainer.transform = .identity
        chevronIcon.transform = CGAffineTransform(rotationAngle: .pi)
      }
    } else {
      if animated {
        UIView.animate(withDuration: 0.14, animations: {
          self.popoverContainer.alpha = 0
          self.popoverContainer.transform = CGAffineTransform(translationX: 0, y: 6).scaledBy(x: 0.96, y: 0.96)
          self.chevronIcon.transform = .identity
        }, completion: { _ in
          if !self.isExpanded {
            self.popoverContainer.isHidden = true
            self.popoverContainer.transform = .identity
          }
        })
      } else {
        popoverContainer.isHidden = true
        popoverContainer.alpha = 0
        popoverContainer.transform = .identity
        chevronIcon.transform = .identity
      }
    }
  }

  private func scheduleAutoHideIfNeeded(for batch: KeyboardSendBatchState) {
    autoHideTask?.cancel()
    autoHideTask = nil
    guard !isExpanded else { return }

    let delayMs: Int
    switch batch.overall {
    case .connecting, .sending:
      return
    case .delivered:
      delayMs = 1900
    case .partial:
      delayMs = 4800
    }

    let expectedID = batch.historyID
    autoHideTask = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .milliseconds(delayMs))
      guard let self, !Task.isCancelled, !self.isExpanded, self.currentBatch?.historyID == expectedID else { return }
      self.dismiss(animated: true)
    }
  }

  @objc private func handlePillTap() {
    guard currentBatch != nil else { return }
    setExpanded(!isExpanded, animated: true)
    if !isExpanded, let batch = currentBatch {
      scheduleAutoHideIfNeeded(for: batch)
    }
  }

  @objc private func handleCollapseTap() {
    collapse()
  }

  override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
    super.traitCollectionDidChange(previousTraitCollection)
    popoverContainer.layer.borderColor = KeyboardTheme.cardBorder.cgColor
    spinner.color = KeyboardTheme.onAccent
    dotsContainer.backgroundColor = KeyboardTheme.onAccent.withAlphaComponent(0.14)
    chevronIcon.tintColor = KeyboardTheme.onAccent.withAlphaComponent(0.66)
    subLabel.textColor = KeyboardTheme.onAccent.withAlphaComponent(0.68)
  }

  // MARK: - Visual Helpers

  private func dotColor(for state: KeyboardSendDeviceState) -> UIColor {
    switch state {
    case .waiting: return UIColor.systemGray3
    case .sending: return KeyboardTheme.tintText
    case .delivered: return KeyboardTheme.tintGreen
    case .offline: return KeyboardTheme.tintAmber
    case .failed: return UIColor.systemRed
    }
  }

  private func stateTextColor(for state: KeyboardSendDeviceState) -> UIColor {
    switch state {
    case .waiting: return .secondaryLabel
    case .sending: return KeyboardTheme.tintText
    case .delivered: return KeyboardTheme.tintGreen
    case .offline: return KeyboardTheme.tintAmber
    case .failed: return UIColor.systemRed
    }
  }

  private func symbolName(for deviceName: String) -> String {
    let lower = deviceName.lowercased()
    if lower.contains("ipad") { return "ipad" }
    if lower.contains("iphone") || lower.contains("phone") || lower.contains("android") || lower.contains("pixel") {
      return "iphone"
    }
    if lower.contains("macbook") || lower.contains("laptop") || lower.contains("book") {
      return "laptopcomputer"
    }
    return "desktopcomputer"
  }
}
