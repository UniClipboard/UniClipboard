import UIKit

@MainActor
final class KeyboardViewController: UIInputViewController {
  private enum Section: Hashable { case main }

  private let store = KeyboardSnapshotStore()
  private lazy var engineSession = KeyboardEngineSession(store: store)
  private var strings = KeyboardStrings()
  private var preferences = KeyboardPreferences.defaults
  private var pasteboardObserver: NSObjectProtocol?
  private var pasteboardPollTimer: Timer?

  private var allItems: [KeyboardClipItem] = []
  private var displayedItems: [KeyboardClipItem] = []
  private var itemsByID: [String: KeyboardClipItem] = [:]
  private var selectedFilter: KeyboardCategoryFilter = .all
  private var loadState: LoadState = .empty
  private var syncIndicator: KeyboardRailView.SyncIndicatorState = .idle
  private var actedItemID: String?
  private var undoStack: [Int] = []

  private enum LoadState {
    case needsFullAccess
    case loaded
    case empty
    case unavailable
  }

  private let rootStack = UIStackView()
  private let bodyStack = UIStackView()
  private let railView = KeyboardRailView()
  private let canvasContainer = UIView()
  private let stateView = KeyboardStateView()
  private let editBarView = KeyboardEditBarView()
  private let previewOverlay = KeyboardPreviewOverlayView()
  private let sendStatusPill = KeyboardSendStatusPillView()
  private let toastContainer = UIView()
  private let toastLabel = UILabel()

  private var heightConstraint: NSLayoutConstraint?
  private var toastTask: Task<Void, Never>?
  private var actedClearTask: Task<Void, Never>?
  private let lightImpact = UIImpactFeedbackGenerator(style: .light)
  private let mediumImpact = UIImpactFeedbackGenerator(style: .medium)

  private lazy var collectionView: UICollectionView = {
    let view = UICollectionView(frame: .zero, collectionViewLayout: makeLayout(for: .all))
    view.translatesAutoresizingMaskIntoConstraints = false
    view.backgroundColor = .clear
    view.showsVerticalScrollIndicator = false
    view.alwaysBounceVertical = true
    view.contentInsetAdjustmentBehavior = .never
    #if compiler(>=6.2)
    if #available(iOS 26.0, *) {
      view.topEdgeEffect.isHidden = true
      view.bottomEdgeEffect.isHidden = true
      view.leftEdgeEffect.isHidden = true
      view.rightEdgeEffect.isHidden = true
    }
    #endif
    view.delegate = self
    view.register(KeyboardListRowCell.self, forCellWithReuseIdentifier: KeyboardListRowCell.reuseIdentifier)
    view.register(KeyboardImageTileCell.self, forCellWithReuseIdentifier: KeyboardImageTileCell.reuseIdentifier)

    let longPress = UILongPressGestureRecognizer(target: self, action: #selector(handleCardLongPress(_:)))
    longPress.minimumPressDuration = 0.35
    view.addGestureRecognizer(longPress)
    return view
  }()

  private lazy var dataSource = UICollectionViewDiffableDataSource<Section, String>(
    collectionView: collectionView
  ) { [weak self] collectionView, indexPath, id in
    guard let self, let item = self.itemsByID[id] else {
      return UICollectionViewCell()
    }
    let thumb = item.thumbnailFile.flatMap { self.store.thumbnailImage(named: $0) }
    let isActed = self.actedItemID == id

    if self.selectedFilter == .image {
      guard let cell = collectionView.dequeueReusableCell(
        withReuseIdentifier: KeyboardImageTileCell.reuseIdentifier,
        for: indexPath
      ) as? KeyboardImageTileCell else { return UICollectionViewCell() }
      cell.configure(item: item, isActed: isActed, strings: self.strings, thumbnail: thumb)
      return cell
    } else {
      guard let cell = collectionView.dequeueReusableCell(
        withReuseIdentifier: KeyboardListRowCell.reuseIdentifier,
        for: indexPath
      ) as? KeyboardListRowCell else { return UICollectionViewCell() }
      let isLatest = (indexPath.item == 0 && self.selectedFilter == .all)
      cell.configure(item: item, isLatest: isLatest, isActed: isActed, strings: self.strings, thumbnail: thumb)
      return cell
    }
  }

  // MARK: - Lifecycle

  override func viewDidLoad() {
    super.viewDidLoad()
    buildHierarchy()
    wireActions()
    wireEngineSession()
    updateHeightConstraint()
  }

  override func viewWillAppear(_ animated: Bool) {
    super.viewWillAppear(animated)
    strings = KeyboardStrings()
    preferences = store.loadPreferences()
    lightImpact.prepare()
    reloadFromStore(showToastOnRefresh: false)
    updateHeightConstraint()
    renderChrome()
  }

  override func viewDidAppear(_ animated: Bool) {
    super.viewDidAppear(animated)
    store.recordHeartbeat(hasFullAccess: hasFullAccess)
    if hasFullAccess {
      engineSession.start(autoSyncEnabled: preferences.autoSyncEnabled, strings: strings)
      if preferences.autoSyncEnabled {
        startPasteboardWatch()
      }
    }
  }

  override func viewWillDisappear(_ animated: Bool) {
    super.viewWillDisappear(animated)
    stopPasteboardWatch()
    sendStatusPill.dismiss(animated: false)
    engineSession.stop()
  }

  override func didReceiveMemoryWarning() {
    super.didReceiveMemoryWarning()
    store.clearImageMemoryCache()
    engineSession.handleMemoryWarning()
  }

  override func textDidChange(_ textInput: (any UITextInput)?) {
    super.textDidChange(textInput)
    renderEditBar()
  }

  // MARK: - Layout & Hierarchy

  private func buildHierarchy() {
    view.backgroundColor = .clear

    rootStack.axis = .vertical
    rootStack.spacing = KeyboardLayoutMetrics.sectionGap
    rootStack.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(rootStack)

    bodyStack.axis = .horizontal
    bodyStack.spacing = KeyboardLayoutMetrics.sectionGap
    bodyStack.alignment = .fill
    bodyStack.translatesAutoresizingMaskIntoConstraints = false

    canvasContainer.translatesAutoresizingMaskIntoConstraints = false
    canvasContainer.addSubview(collectionView)
    canvasContainer.addSubview(stateView)

    bodyStack.addArrangedSubview(railView)
    bodyStack.addArrangedSubview(canvasContainer)

    rootStack.addArrangedSubview(bodyStack)
    rootStack.addArrangedSubview(editBarView)

    // Non-blocking bottom toast anchored above edit bar (never covers #1 latest item).
    toastContainer.translatesAutoresizingMaskIntoConstraints = false
    toastContainer.backgroundColor = KeyboardTheme.accent
    toastContainer.layer.cornerRadius = 14
    toastContainer.layer.cornerCurve = .continuous
    toastContainer.isUserInteractionEnabled = false
    toastContainer.isHidden = true
    toastLabel.translatesAutoresizingMaskIntoConstraints = false
    toastLabel.font = .systemFont(ofSize: 11.5, weight: .semibold)
    toastLabel.textColor = KeyboardTheme.onAccent
    toastContainer.addSubview(toastLabel)
    view.addSubview(sendStatusPill)
    view.addSubview(toastContainer)

    view.addSubview(previewOverlay)

    NSLayoutConstraint.activate([
      rootStack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: KeyboardLayoutMetrics.outerPaddingHorizontal),
      rootStack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -KeyboardLayoutMetrics.outerPaddingHorizontal),
      rootStack.topAnchor.constraint(equalTo: view.topAnchor, constant: KeyboardLayoutMetrics.outerPaddingTop),
      rootStack.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -KeyboardLayoutMetrics.outerPaddingBottom),

      collectionView.leadingAnchor.constraint(equalTo: canvasContainer.leadingAnchor),
      collectionView.trailingAnchor.constraint(equalTo: canvasContainer.trailingAnchor),
      collectionView.topAnchor.constraint(equalTo: canvasContainer.topAnchor),
      collectionView.bottomAnchor.constraint(equalTo: canvasContainer.bottomAnchor),

      stateView.leadingAnchor.constraint(equalTo: canvasContainer.leadingAnchor),
      stateView.trailingAnchor.constraint(equalTo: canvasContainer.trailingAnchor),
      stateView.topAnchor.constraint(equalTo: canvasContainer.topAnchor),
      stateView.bottomAnchor.constraint(equalTo: canvasContainer.bottomAnchor),

      sendStatusPill.leadingAnchor.constraint(equalTo: canvasContainer.leadingAnchor, constant: 2),
      sendStatusPill.trailingAnchor.constraint(equalTo: canvasContainer.trailingAnchor, constant: -2),
      sendStatusPill.topAnchor.constraint(greaterThanOrEqualTo: rootStack.topAnchor),
      sendStatusPill.bottomAnchor.constraint(equalTo: editBarView.topAnchor, constant: -6),

      toastLabel.leadingAnchor.constraint(equalTo: toastContainer.leadingAnchor, constant: 12),
      toastLabel.trailingAnchor.constraint(equalTo: toastContainer.trailingAnchor, constant: -12),
      toastLabel.topAnchor.constraint(equalTo: toastContainer.topAnchor, constant: 5),
      toastLabel.bottomAnchor.constraint(equalTo: toastContainer.bottomAnchor, constant: -5),
      toastContainer.centerXAnchor.constraint(equalTo: view.centerXAnchor),
      toastContainer.bottomAnchor.constraint(equalTo: editBarView.topAnchor, constant: -8),

      previewOverlay.leadingAnchor.constraint(equalTo: view.leadingAnchor),
      previewOverlay.trailingAnchor.constraint(equalTo: view.trailingAnchor),
      previewOverlay.topAnchor.constraint(equalTo: view.topAnchor),
      previewOverlay.bottomAnchor.constraint(equalTo: view.bottomAnchor),
    ])
  }

  private func updateHeightConstraint() {
    let target = KeyboardLayoutMetrics.targetHeight(
      hasFullAccess: hasFullAccess,
      needsInputModeSwitchKey: needsInputModeSwitchKey
    )
    if let existing = heightConstraint {
      existing.constant = target
    } else {
      let c = view.heightAnchor.constraint(equalToConstant: target)
      c.priority = .required - 1
      c.isActive = true
      heightConstraint = c
    }
  }

  private func makeLayout(for filter: KeyboardCategoryFilter) -> UICollectionViewLayout {
    if filter == .image {
      let itemSize = NSCollectionLayoutSize(widthDimension: .fractionalWidth(0.5), heightDimension: .absolute(106))
      let item = NSCollectionLayoutItem(layoutSize: itemSize)
      let groupSize = NSCollectionLayoutSize(widthDimension: .fractionalWidth(1.0), heightDimension: .absolute(106))
      let group = NSCollectionLayoutGroup.horizontal(layoutSize: groupSize, repeatingSubitem: item, count: 2)
      group.interItemSpacing = .fixed(7)
      let section = NSCollectionLayoutSection(group: group)
      section.interGroupSpacing = 7
      section.contentInsets = NSDirectionalEdgeInsets(top: 1, leading: 1, bottom: 4, trailing: 1)
      return UICollectionViewCompositionalLayout(section: section)
    } else {
      let itemSize = NSCollectionLayoutSize(widthDimension: .fractionalWidth(1.0), heightDimension: .estimated(62))
      let item = NSCollectionLayoutItem(layoutSize: itemSize)
      let groupSize = NSCollectionLayoutSize(widthDimension: .fractionalWidth(1.0), heightDimension: .estimated(62))
      let group = NSCollectionLayoutGroup.vertical(layoutSize: groupSize, subitems: [item])
      let section = NSCollectionLayoutSection(group: group)
      section.interGroupSpacing = 6
      section.contentInsets = NSDirectionalEdgeInsets(top: 1, leading: 1, bottom: 4, trailing: 1)
      return UICollectionViewCompositionalLayout(section: section)
    }
  }

  // MARK: - Action Wiring

  private func wireActions() {
    railView.onSelectFilter = { [weak self] filter in
      guard let self else { return }
      self.triggerFeedback()
      guard filter != self.selectedFilter else { return }
      self.selectedFilter = filter
      self.previewOverlay.dismiss()
      self.sendStatusPill.collapse()
      self.collectionView.setCollectionViewLayout(self.makeLayout(for: filter), animated: false)
      self.applyFilteredItems(scrollToTop: true)
      self.renderRail()
    }

    railView.onTapRefresh = { [weak self] in
      guard let self else { return }
      self.triggerFeedback()
      self.syncIndicator = .syncing
      self.renderRail()
      if self.engineSession.isLive {
        self.engineSession.manualRefresh()
      } else {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { [weak self] in
          self?.reloadFromStore(showToastOnRefresh: true)
        }
      }
    }

    editBarView.onUndo = { [weak self] in
      guard let self else { return }
      self.triggerFeedback()
      self.performUndo()
    }

    editBarView.onSpace = { [weak self] in
      guard let self else { return }
      self.triggerFeedback()
      self.textDocumentProxy.insertText(" ")
      self.undoStack.append(1)
      self.renderEditBar()
    }

    editBarView.onScrubCursor = { [weak self] offset in
      guard let self else { return }
      self.textDocumentProxy.adjustTextPosition(byCharacterOffset: offset)
    }

    editBarView.onReturn = { [weak self] in
      guard let self else { return }
      self.triggerFeedback()
      self.textDocumentProxy.insertText("\n")
      self.undoStack.removeAll()
      self.renderEditBar()
    }

    editBarView.onDeleteBackward = { [weak self] isRepeating in
      guard let self else { return }
      if !isRepeating { self.triggerFeedback() }
      self.textDocumentProxy.deleteBackward()
      self.undoStack.removeAll()
      self.renderEditBar()
    }

    editBarView.onAdvanceInputMode = { [weak self] in
      self?.advanceToNextInputMode()
    }

    previewOverlay.onClose = { [weak self] in
      self?.previewOverlay.dismiss()
    }

    previewOverlay.onTogglePin = { [weak self] item in
      guard let self else { return }
      self.triggerFeedback()
      self.togglePin(itemID: item.id)
    }

    previewOverlay.onPrimaryAction = { [weak self] item in
      guard let self else { return }
      self.previewOverlay.dismiss()
      self.activateItem(item)
    }

    sendStatusPill.onRetryDevice = { [weak self] historyID, deviceID in
      guard let self else { return }
      self.triggerFeedback()
      self.engineSession.retrySend(historyID: historyID, deviceID: deviceID)
    }
  }

  private func wireEngineSession() {
    engineSession.onStateChanged = { [weak self] state in
      guard let self else { return }
      switch state {
      case .connecting:
        self.syncIndicator = .syncing
      case .connected, .degraded, .idle:
        self.syncIndicator = .idle
      case .error:
        self.syncIndicator = .error
      }
      self.renderRail()
    }
    engineSession.onItemsUpdated = { [weak self] items in
      guard let self, self.hasFullAccess else { return }
      self.allItems = items
      self.loadState = items.isEmpty ? .empty : .loaded
      self.renderChrome()
      self.applyFilteredItems(scrollToTop: false)
    }
    engineSession.onToast = { [weak self] text in
      self?.showToast(text)
    }
    engineSession.onSendBatchUpdated = { [weak self] batch in
      guard let self else { return }
      if let batch {
        self.sendStatusPill.update(batch: batch, strings: self.strings)
      } else {
        self.sendStatusPill.dismiss(animated: false)
      }
    }
  }

  private func startPasteboardWatch() {
    stopPasteboardWatch()
    pasteboardObserver = NotificationCenter.default.addObserver(
      forName: UIPasteboard.changedNotification,
      object: nil,
      queue: .main
    ) { [weak self] _ in
      Task { @MainActor [weak self] in
        self?.engineSession.checkClipboardChange()
      }
    }
    pasteboardPollTimer = Timer.scheduledTimer(withTimeInterval: 0.6, repeats: true) { [weak self] _ in
      Task { @MainActor [weak self] in
        self?.engineSession.checkClipboardChange()
      }
    }
  }

  private func stopPasteboardWatch() {
    if let obs = pasteboardObserver {
      NotificationCenter.default.removeObserver(obs)
      pasteboardObserver = nil
    }
    pasteboardPollTimer?.invalidate()
    pasteboardPollTimer = nil
  }

  // MARK: - Store Loading & Rendering

  private func reloadFromStore(showToastOnRefresh: Bool) {
    guard hasFullAccess else {
      loadState = .needsFullAccess
      allItems = []
      displayedItems = []
      itemsByID = [:]
      syncIndicator = .idle
      renderChrome()
      applyFilteredItems(scrollToTop: false)
      return
    }

    store.recordHeartbeat(hasFullAccess: true)
    let result = store.loadSnapshot()
    switch result {
    case .loaded(let items, _, _):
      allItems = items
      loadState = .loaded
      syncIndicator = .idle
      if showToastOnRefresh {
        showToast(strings.toastSynced)
      }
    case .empty:
      allItems = []
      loadState = .empty
      syncIndicator = .idle
      if showToastOnRefresh {
        showToast(strings.toastSynced)
      }
    case .unavailable:
      allItems = []
      loadState = .unavailable
      syncIndicator = .error
    }

    renderChrome()
    applyFilteredItems(scrollToTop: false)
  }

  private func renderChrome() {
    let isGate = !hasFullAccess
    railView.isHidden = isGate
    renderRail()
    renderEditBar()
  }

  private func renderRail() {
    let pinnedCount = allItems.filter(\.pinned).count
    railView.configure(
      selectedFilter: selectedFilter,
      pinnedCount: pinnedCount,
      syncState: syncIndicator,
      strings: strings
    )
  }

  private func renderEditBar() {
    let returnType = textDocumentProxy.returnKeyType ?? .default
    editBarView.configure(
      canUndo: !undoStack.isEmpty,
      returnTitle: strings.returnTitle(for: returnType),
      showsEditRow: hasFullAccess,
      showsGlobeBand: needsInputModeSwitchKey,
      strings: strings
    )
  }

  private func applyFilteredItems(scrollToTop: Bool) {
    if !hasFullAccess {
      collectionView.isHidden = true
      stateView.isHidden = false
      stateView.configure(
        symbol: "lock.shield",
        title: strings.gateTitle,
        message: strings.gateMessage,
        chips: [strings.gateChipNoKeylog, strings.gateChipLocalSync],
        actionTitle: strings.gateOpenSettings,
        action: { [weak self] in self?.openSystemSettings() }
      )
      return
    }

    if loadState == .unavailable {
      collectionView.isHidden = true
      stateView.isHidden = false
      stateView.configure(
        symbol: "exclamationmark.triangle",
        symbolColor: KeyboardTheme.tintAmber,
        title: strings.errorTitle,
        message: strings.errorMessage,
        actionTitle: strings.retryButton,
        action: { [weak self] in self?.reloadFromStore(showToastOnRefresh: true) }
      )
      return
    }

    let filtered = allItems.filter { selectedFilter.matches($0) }
    displayedItems = filtered
    itemsByID = Dictionary(uniqueKeysWithValues: filtered.map { ($0.id, $0) })

    if filtered.isEmpty {
      collectionView.isHidden = true
      stateView.isHidden = false
      let emptyInfo: (symbol: String, title: String, message: String)
      switch selectedFilter {
      case .all: emptyInfo = ("tray", strings.emptyAllTitle, strings.emptyAllMessage)
      case .pinned: emptyInfo = ("pin", strings.emptyPinnedTitle, strings.emptyPinnedMessage)
      case .url: emptyInfo = ("link", strings.emptyLinksTitle, strings.emptyAllMessage)
      case .image: emptyInfo = ("photo", strings.emptyImagesTitle, strings.emptyAllMessage)
      }
      stateView.configure(
        symbol: emptyInfo.symbol,
        title: emptyInfo.title,
        message: emptyInfo.message
      )
      return
    }

    stateView.isHidden = true
    collectionView.isHidden = false
    var snapshot = NSDiffableDataSourceSnapshot<Section, String>()
    snapshot.appendSections([.main])
    snapshot.appendItems(filtered.map(\.id), toSection: .main)
    dataSource.applySnapshotUsingReloadData(snapshot)
    if scrollToTop {
      collectionView.setContentOffset(.zero, animated: false)
    }
  }

  // MARK: - Item Activation, Undo, Pin & Preview

  private func activateItem(_ item: KeyboardClipItem) {
    triggerFeedback()
    flashActed(itemID: item.id)

    if item.kind == .image {
      if let image = store.copyableImage(for: item) {
        UIPasteboard.general.image = image
        let afterCC = UIPasteboard.general.changeCount
        let hash = image.pngData().map { KeyboardSnapshotStore.sha256Hex(of: $0) } ?? ""
        store.recordPasteboardState(changeCount: afterCC, contentSHA256: hash, origin: "applied")
        showToast(strings.toastImageCopied)
      } else {
        showToast(strings.toastImageUnavailable)
      }
      return
    }

    let text = item.insertText ?? item.title
    guard !text.isEmpty else { return }
    textDocumentProxy.insertText(text)
    undoStack.append(text.count)
    renderEditBar()
    showToast(item.kind == .otp ? strings.toastOTPInserted : strings.toastInserted)
  }

  private func performUndo() {
    guard let count = undoStack.popLast(), count > 0 else {
      showToast(strings.toastNothingToUndo)
      return
    }
    for _ in 0..<count {
      textDocumentProxy.deleteBackward()
    }
    renderEditBar()
    showToast(strings.toastUndone)
  }

  private func togglePin(itemID: String) {
    guard let idx = allItems.firstIndex(where: { $0.id == itemID }) else { return }
    allItems[idx].pinned.toggle()
    let updated = allItems[idx]
    previewOverlay.dismiss()
    renderRail()
    applyFilteredItems(scrollToTop: false)
    showToast(updated.pinned ? strings.toastPinned : strings.toastUnpinned)
    engineSession.togglePin(historyID: updated.id, pinned: updated.pinned) { [weak self] handled in
      if !handled {
        self?.store.enqueuePinChange(historyID: updated.id, pinned: updated.pinned)
      }
    }
  }

  @objc private func handleCardLongPress(_ gesture: UILongPressGestureRecognizer) {
    guard gesture.state == .began else { return }
    sendStatusPill.collapse()
    let point = gesture.location(in: collectionView)
    guard let indexPath = collectionView.indexPathForItem(at: point),
          indexPath.item < displayedItems.count else { return }
    let item = displayedItems[indexPath.item]
    if preferences.hapticEnabled {
      mediumImpact.impactOccurred()
    }
    let image = item.kind == .image ? store.copyableImage(for: item) : nil
    previewOverlay.present(item: item, image: image, strings: strings)
  }

  private func flashActed(itemID: String) {
    actedItemID = itemID
    reconfigureItem(itemID)
    actedClearTask?.cancel()
    actedClearTask = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .milliseconds(650))
      guard let self, !Task.isCancelled else { return }
      let prev = self.actedItemID
      self.actedItemID = nil
      if let prev { self.reconfigureItem(prev) }
    }
  }

  private func reconfigureItem(_ id: String) {
    var snap = dataSource.snapshot()
    guard snap.itemIdentifiers.contains(id) else { return }
    snap.reconfigureItems([id])
    dataSource.apply(snap, animatingDifferences: false)
  }

  private func showToast(_ text: String) {
    sendStatusPill.setTemporarilySuppressed(true)
    toastLabel.text = text
    toastContainer.isHidden = false
    toastContainer.alpha = 0
    toastContainer.transform = CGAffineTransform(translationX: 0, y: 6)
    UIView.animate(withDuration: 0.16) {
      self.toastContainer.alpha = 1
      self.toastContainer.transform = .identity
    }
    toastTask?.cancel()
    toastTask = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .milliseconds(1450))
      guard let self, !Task.isCancelled else { return }
      UIView.animate(withDuration: 0.2, animations: {
        self.toastContainer.alpha = 0
      }, completion: { _ in
        if self.toastContainer.alpha == 0 {
          self.toastContainer.isHidden = true
          self.sendStatusPill.setTemporarilySuppressed(false)
        }
      })
    }
  }

  private func triggerFeedback() {
    if preferences.soundEnabled {
      UIDevice.current.playInputClick()
    }
    if preferences.hapticEnabled {
      lightImpact.impactOccurred()
    }
  }

  private func openSystemSettings() {
    guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
    var responder: UIResponder? = self
    let selector = sel_registerName("openURL:")
    while let current = responder {
      if current.responds(to: selector) {
        _ = current.perform(selector, with: url)
        return
      }
      responder = current.next
    }
  }
}

extension KeyboardViewController: UICollectionViewDelegate {
  func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
    sendStatusPill.collapse()
    guard indexPath.item < displayedItems.count else { return }
    activateItem(displayedItems[indexPath.item])
  }

  func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
    sendStatusPill.collapse()
  }
}
