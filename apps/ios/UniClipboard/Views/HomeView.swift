import SwiftUI
import UIKit

/// Home tab — two-column grid of clipboard history cards (Paste-app style).
///
/// Replaces the earlier List-based layout with a `LazyVGrid` that shows
/// `ClipboardCard` cells. Supports search, multi-select, long-press preview
/// overlay, and a bottom toolbar for search / server picker / sync.
struct HomeView: View {
    @Bindable var vm: AppViewModel

    /// Injected by `ContentView` so the error-detail sheet's "去设置" CTA
    /// can flip the TabView selection without HomeView knowing about it.
    var onGoToSettings: () -> Void = {}

    // MARK: - State

    @State private var showingErrorSheet: Bool =
        ProcessInfo.processInfo.environment["UC_OPEN_ISSUE_SHEET"] == "1"
    @State private var pinnedItemId: UUID?
    @State private var loadingItemIds: Set<UUID> = []
    @State private var thumbnailCache: [UUID: UIImage] = [:]
    @State private var urlMetadataCache: [UUID: URLCardMetadata] = [:]
    @State private var isSearching: Bool = false
    @State private var searchText: String = ""
    @FocusState private var isSearchFocused: Bool
    @State private var filterTypes: Set<ClipboardDisplayKind> = []
    @State private var filterDate: SearchDateFilter = .all
    @State private var showingFilterSheet: Bool = false
    @State private var isSelectMode: Bool = false
    @State private var selectedIds: Set<UUID> = []
    @State private var showingDeleteConfirm: Bool = false
    @State private var showingServerPicker: Bool =
        ProcessInfo.processInfo.environment["UC_OPEN_SWITCHER"] == "1"

    /// Shared identity space for Liquid Glass morphing in the bottom bar. The
    /// default toolbar's search circle and the search field's capsule both tag
    /// themselves "search" in this namespace, so they liquid-morph into each
    /// other when search opens/closes (see `bottomBar`).
    @Namespace private var glassNS

    // MARK: - Derived

    private var engineState: SyncEngine.State { vm.engine.state }
    private var isExplicitlyRefreshing: Bool { vm.engine.isExplicitlyRefreshing }

    private var showPasteHint: Bool {
        vm.appSettings.autoPushDeviceChanges && !vm.appSettings.pastePermissionHintDismissed
    }

    /// History rendered in the grid, sorted newest-first. When a card is
    /// pinned (just tapped-to-copy), that item is promoted to position 0
    /// regardless of its timestamp so the user sees the visual feedback.
    private var sortedHistory: [ClipboardHistoryItem] {
        var items = vm.history.sorted { $0.timestamp > $1.timestamp }
        if let pinId = pinnedItemId,
           let idx = items.firstIndex(where: { $0.id == pinId }), idx > 0 {
            let pinned = items.remove(at: idx)
            items.insert(pinned, at: 0)
        }
        return items
    }

    private var latestId: UUID? {
        vm.history.sorted { $0.timestamp > $1.timestamp }.first?.id
    }

    private var hasActiveFilters: Bool {
        !filterTypes.isEmpty || filterDate != .all
    }

    /// True when every currently-displayed row is selected. Drives the
    /// 全选 / 取消全选 label and is `false` when there's nothing to select.
    private var allDisplayedSelected: Bool {
        !displayedHistory.isEmpty && selectedIds.count == displayedHistory.count
    }

    /// Items displayed in the grid, filtered by search text + type + date.
    private var displayedHistory: [ClipboardHistoryItem] {
        var items = sortedHistory

        if isSearching {
            if !searchText.isEmpty {
                let query = searchText.lowercased()
                items = items.filter {
                    $0.entry.text.lowercased().contains(query)
                    || ($0.entry.dataName?.lowercased().contains(query) ?? false)
                }
            }
            if !filterTypes.isEmpty {
                items = items.filter { filterTypes.contains($0.entry.displayKind) }
            }
            items = filterDate.apply(to: items)
        }

        return items
    }

    private let twoFlexibleColumns = [
        GridItem(.flexible(), spacing: 12),
        GridItem(.flexible(), spacing: 12)
    ]

    // MARK: - Body

    var body: some View {
        gridOrEmpty
            .safeAreaInset(edge: .top, spacing: 0) {
                VStack(spacing: 0) {
                    customTopBar
                    if showPasteHint {
                        PastePermissionBanner(
                            onOpenSettings: { openAppSettings() },
                            onDismiss: { vm.appSettings.pastePermissionHintDismissed = true }
                        )
                    }
                }
            }
            .animation(.snappy, value: showPasteHint)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                bottomBar
            }
            .refreshable {
                await vm.engine.explicitRefresh()
            }
            .scrollContentBackground(.hidden)
            .background(Color(.systemGroupedBackground))
            .toolbar(.hidden, for: .navigationBar)
            .sheet(isPresented: $showingServerPicker) {
                ServerSwitcherSheet(
                    vm: vm,
                    onSelect: { id in
                        vm.setActiveServer(id)
                        showingServerPicker = false
                    }
                )
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $showingErrorSheet) {
                if let issue = currentIssue {
                    IssueDetailSheet(
                        issue: issue,
                        onPrimaryAction: {
                            showingErrorSheet = false
                            switch issue.primaryAction {
                            case .goToSettings:    onGoToSettings()
                            case .retry:           vm.engine.forceTickNow()
                            case .acknowledgeLoop: vm.engine.acknowledgeLoopDetection()
                            case .dismiss:         break
                            }
                        }
                    )
                    .presentationDetents([.medium])
                    .presentationDragIndicator(.visible)
                }
            }
    }

    // MARK: - Top Bar

    @ViewBuilder
    private var customTopBar: some View {
        if isSelectMode {
            selectModeTopBar
        } else {
            defaultTopBar
        }
    }

    private var defaultTopBar: some View {
        HStack(spacing: 12) {
            // Left: server status (read-only)
            HStack(spacing: 6) {
                Circle()
                    .fill(.green)
                    .frame(width: 6, height: 6)
                Text(verbatim: vm.activeServer?.displayLabel ?? String(localized: "未配置"))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            // Trailing interactive glass cluster — grouped so their
            // morph/blur sampling are coordinated on iOS 26+.
            GlassGroup {
                HStack(spacing: 12) {
                    // Error badge (if any)
                    if let issue = currentIssue {
                        Button {
                            showingErrorSheet = true
                        } label: {
                            Image(systemName: "exclamationmark.circle.fill")
                                .font(.title2)
                                .foregroundStyle(issue.tint)
                                .frame(width: 52, height: 52)
                                .liquidGlassCircle(interactive: true)
                        }
                        .accessibilityLabel(Text(issue.title))
                    }

                    // "选择" capsule
                    Button {
                        isSelectMode = true
                        selectedIds = []
                    } label: {
                        Text("选择")
                            .font(.subheadline.weight(.medium))
                            .lineLimit(1)
                            .fixedSize(horizontal: true, vertical: false)
                            .frame(height: 52)
                            .padding(.horizontal, 20)
                            .liquidGlassCapsule(interactive: true)
                    }

                    // "⋯" menu circle
                    Menu {
                        Button {
                            onGoToSettings()
                        } label: {
                            Label("设置", systemImage: "gearshape")
                        }
                    } label: {
                        Image(systemName: "ellipsis")
                            .font(.title2)
                            .frame(width: 52, height: 52)
                            .liquidGlassCircle(interactive: true)
                    }
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
    }

    /// Select-mode variant: shows the selected count on the left and a
    /// 全选/取消全选 + 完成 pair on the right. Server dot, error badge, and
    /// the ⋯ menu are hidden while selecting.
    private var selectModeTopBar: some View {
        HStack(spacing: 12) {
            Text("已选择 \(selectedIds.count) 项")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.primary)
                .lineLimit(1)

            Spacer(minLength: 0)

            GlassGroup {
                HStack(spacing: 12) {
                    Button {
                        selectAll()
                    } label: {
                        Text(allDisplayedSelected ? "取消全选" : "全选")
                            .font(.subheadline.weight(.medium))
                            .lineLimit(1)
                            .fixedSize(horizontal: true, vertical: false)
                            .frame(height: 52)
                            .padding(.horizontal, 20)
                            .liquidGlassCapsule(interactive: true)
                    }

                    Button {
                        exitSelectMode()
                    } label: {
                        Text("完成")
                            .font(.subheadline.weight(.medium))
                            .lineLimit(1)
                            .fixedSize(horizontal: true, vertical: false)
                            .frame(height: 52)
                            .padding(.horizontal, 20)
                            .liquidGlassCapsule(interactive: true)
                    }
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
    }

    // MARK: - Grid or Empty

    @ViewBuilder
    private var gridOrEmpty: some View {
        if sortedHistory.isEmpty {
            // Wrap the empty state in a ScrollView so `.refreshable` (attached
            // on the body) actually engages: ContentUnavailableView is not
            // scrollable on its own, so pull-to-refresh never fired here —
            // exactly when the user most wants to retry.
            GeometryReader { proxy in
                ScrollView(.vertical) {
                    ContentUnavailableView {
                        Label {
                            Text("还没有同步过剪贴板")
                        } icon: {
                            Image(systemName: "doc.on.clipboard")
                        }
                    } description: {
                        Text("服务器的新内容会自动出现在这里；本机内容点下方按钮推送")
                    } actions: {
                        PasteButton(supportedContentTypes: PastedItemExtractor.supportedContentTypes) { providers in
                            Task { await vm.pushPastedProviders(providers) }
                        }
                        .tint(.accentColor)
                        .accessibilityLabel(Text("推送本机剪贴板到服务器"))
                    }
                    .frame(minHeight: proxy.size.height)
                }
            }
        } else {
            ScrollView(.vertical) {
                LazyVGrid(columns: twoFlexibleColumns, spacing: 12) {
                    ForEach(displayedHistory) { item in
                        cardCell(for: item)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 8)
                .padding(.bottom, 16)
            }
            .scrollEdgeEffectStyleSoftTopBottom()
            .animation(.snappy, value: pinnedItemId)
            // Fire a light haptic when a successful copy promotes an item to
            // the pin slot. Replaces the old per-call UIImpactFeedbackGenerator.
            .sensoryFeedback(.impact(weight: .light), trigger: pinnedItemId)
            .task { preloadThumbnails() }
            // Watch the array itself, not just its count: a push completion
            // flips the head row's direction IN PLACE (same-hash dedup), and
            // that's exactly the moment a previously-failed thumbnail load
            // (it raced the PUT) becomes satisfiable from the local cache.
            .onChange(of: vm.history) { _, _ in preloadThumbnails() }
        }
    }

    // MARK: - Search Bottom Bar

    private var searchBottomBar: some View {
        VStack(spacing: 8) {
            // Filter tags row (when filters are active)
            if hasActiveFilters {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(Array(filterTypes), id: \.self) { kind in
                            filterTag(label: kind.localizedLabelString) {
                                filterTypes.remove(kind)
                            }
                        }
                        if filterDate != .all {
                            filterTag(label: filterDate.label) {
                                filterDate = .all
                            }
                        }
                    }
                    .padding(.horizontal, 16)
                }
            }

            // Search input row — the input capsule is NOT interactive glass
            // (it hosts a text field, not a button); the filter and close
            // circles are buttons, so they get interactive glass. No inner
            // GlassGroup here: these live in bottomBar's unified container, so
            // the capsule can morph in from the toolbar's "search" circle.
            HStack(spacing: 8) {
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                        .font(.subheadline)
                    TextField(String(localized: "搜索剪贴板"), text: $searchText)
                        .font(.subheadline)
                        .textFieldStyle(.plain)
                        .focused($isSearchFocused)
                        // Focus on appear instead of a hard-coded delay.
                        .onAppear { isSearchFocused = true }
                    if !searchText.isEmpty {
                        Button {
                            searchText = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                                .font(.caption)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(Text("清除搜索"))
                    }
                }
                .frame(height: 52)
                .padding(.horizontal, 12)
                .liquidGlassCapsule()
                .glassMorphID("search", in: glassNS)

                Button {
                    showingFilterSheet = true
                } label: {
                    Image(systemName: hasActiveFilters ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle")
                        .font(.title2)
                        .foregroundStyle(hasActiveFilters ? Color.accentColor : .primary)
                        .frame(width: 52, height: 52)
                        .liquidGlassCircle(interactive: true)
                }
                .accessibilityLabel(Text("筛选"))

                Button {
                    withAnimation(.snappy) {
                        isSearching = false
                        searchText = ""
                        filterTypes = []
                        filterDate = .all
                    }
                } label: {
                    Image(systemName: "xmark")
                        .font(.title2)
                        .frame(width: 52, height: 52)
                        .liquidGlassCircle(interactive: true)
                }
                .accessibilityLabel(Text("关闭搜索"))
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 8)
        .sheet(isPresented: $showingFilterSheet) {
            SearchFilterSheet(
                selectedTypes: $filterTypes,
                selectedDate: $filterDate
            )
            .presentationDetents([.medium])
            .presentationDragIndicator(.visible)
        }
    }

    private func filterTag(label: String, onRemove: @escaping () -> Void) -> some View {
        // The whole tag is the hit target — tapping anywhere removes the
        // filter. `.contentShape(Capsule())` + a 44pt minHeight give a
        // comfortable target while staying visually compact.
        Button(action: onRemove) {
            HStack(spacing: 4) {
                Text(label)
                    .font(.caption.weight(.medium))
                Image(systemName: "xmark")
                    .font(.caption2.weight(.bold))
            }
            .padding(.horizontal, 12)
            .frame(minHeight: 44)
            .contentShape(Capsule())
            .background(Color.accentColor.opacity(0.15), in: Capsule())
            .foregroundStyle(Color.accentColor)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("移除筛选 \(label)"))
    }

    // MARK: - Card Cell

    @ViewBuilder
    private func cardCell(for item: ClipboardHistoryItem) -> some View {
        let isSelected = isSelectMode && selectedIds.contains(item.id)

        // Wrap the card in a Button so it gains the button accessibility
        // trait + press feedback that `.onTapGesture` lacked. `.plain`
        // preserves the card's own visuals. The select overlay, contextMenu,
        // and `.task` loaders all stay attached to the button.
        Button {
            if isSelectMode {
                toggleSelection(item.id)
            } else {
                handleTapToCopy(item)
            }
        } label: {
            ClipboardCard(
                item: item,
                isLatest: item.id == latestId,
                thumbnailImage: thumbnailCache[item.id],
                urlMetadata: urlMetadataCache[item.id],
                isLoading: loadingItemIds.contains(item.id)
            )
            .overlay {
                if isSelectMode {
                    ZStack {
                        if isSelected {
                            RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .stroke(Color.accentColor, lineWidth: 2)
                        }
                        Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                            .font(.system(size: 28))
                            .foregroundStyle(isSelected ? Color.accentColor : Color.secondary.opacity(0.6))
                            .transition(.scale.combined(with: .opacity))
                    }
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(cardAccessibilityLabel(for: item)))
        .contextMenu {
            contextMenuItems(for: item)
        } preview: {
            CardPreviewView(item: item, vm: vm)
        }
        .task {
            await loadThumbnailIfNeeded(for: item)
            await loadURLMetadataIfNeeded(for: item)
        }
        .animation(.snappy, value: isSelectMode)
        .animation(.snappy, value: isSelected)
    }

    /// VoiceOver label for a card: kind + a short content snippet.
    private func cardAccessibilityLabel(for item: ClipboardHistoryItem) -> String {
        let kind = item.entry.displayKind.localizedLabelString
        let snippetSource: String
        switch item.entry.displayKind {
        case .file, .group:
            snippetSource = item.entry.dataName ?? item.entry.text
        default:
            snippetSource = item.entry.text
        }
        let snippet = String(snippetSource.prefix(40))
        // Both pieces are already runtime values (kind is localized; snippet is
        // user content), so this is plain interpolation — not a catalog key.
        return "\(kind):\(snippet)"
    }

    // MARK: - Bottom Bar

    @ViewBuilder
    /// One unified `GlassEffectContainer` wraps all three bottom-bar states so
    /// glass elements that share a `glassMorphID` across states (the search
    /// circle ↔ search capsule) liquid-morph instead of cross-fading. The inner
    /// states must NOT nest their own containers, or the shared id can't morph
    /// across them.
    private var bottomBar: some View {
        GlassGroup {
            bottomBarContent
        }
    }

    @ViewBuilder
    private var bottomBarContent: some View {
        if isSelectMode {
            selectModeBottomBar
        } else if isSearching {
            searchBottomBar
        } else {
            HomeBottomToolbar(
                serverLabel: vm.activeServer?.displayLabel ?? String(localized: "未配置"),
                isAutoSwitched: vm.activeServer?.id != vm.servers.activeConfig?.id,
                isSyncing: isExplicitlyRefreshing,
                glassNamespace: glassNS,
                onSearch: {
                    withAnimation(.snappy) {
                        isSearching = true
                    }
                },
                onServerPicker: {
                    showingServerPicker = true
                },
                onSync: {
                    handleSync()
                }
            )
        }
    }

    private var selectModeBottomBar: some View {
        // No inner GlassGroup: select mode lives in bottomBar's unified
        // container alongside the other states (it has no morphing element).
        HStack(spacing: 24) {
            Spacer()

            Button {
                batchCopy()
            } label: {
                Image(systemName: "doc.on.doc")
                    .font(.title2)
                    .frame(width: 52, height: 52)
                    .liquidGlassCircle(interactive: true)
            }
            .disabled(selectedIds.isEmpty)
            .accessibilityLabel(Text("复制所选"))

            // Share the joined text of the selected rows. ShareLink
            // replaces the old UIActivityViewController window walk.
            ShareLink(item: selectedJoinedText) {
                Image(systemName: "square.and.arrow.up")
                    .font(.title2)
                    .frame(width: 52, height: 52)
                    .liquidGlassCircle(interactive: true)
            }
            .disabled(selectedIds.isEmpty)
            .accessibilityLabel(Text("分享所选"))

            Button {
                showingDeleteConfirm = true
            } label: {
                Image(systemName: "trash")
                    .font(.title2)
                    .foregroundStyle(.red)
                    .frame(width: 52, height: 52)
                    .liquidGlassCircle(interactive: true)
            }
            .disabled(selectedIds.isEmpty)
            .accessibilityLabel(Text("删除所选"))

            Spacer()
        }
        .padding(.vertical, 10)
        .confirmationDialog(
            Text("删除 \(selectedIds.count) 项?"),
            isPresented: $showingDeleteConfirm,
            titleVisibility: .visible
        ) {
            Button("删除", role: .destructive) {
                batchDelete()
            }
            Button("取消", role: .cancel) {}
        }
    }

    /// Joined text of currently-selected rows, newest-first. Used by the
    /// select-mode ShareLink and as a stable item for `ShareLink(item:)`.
    private var selectedJoinedText: String {
        displayedHistory
            .filter { selectedIds.contains($0.id) }
            .map(\.entry.text)
            .joined(separator: "\n")
    }

    // MARK: - Actions

    private func handleTapToCopy(_ item: ClipboardHistoryItem) {
        switch item.entry.displayKind {
        case .text, .url:
            vm.reapplyHistoryItem(item)
            withAnimation(.snappy) {
                pinnedItemId = item.id
            }
            // Haptic fires via `.sensoryFeedback(trigger: pinnedItemId)` on
            // the grid — setting the pin above is the trigger.
            schedulePinReset()
            Task { await vm.pushHistoryEntryToServer(item) }

        case .image:
            guard item.entry.hasData else { return }
            loadingItemIds.insert(item.id)
            Task {
                await vm.applyAttachment(for: item)
                loadingItemIds.remove(item.id)
                guard vm.applyError == nil else { return }
                vm.reapplyHistoryItem(item)
                withAnimation(.snappy) {
                    pinnedItemId = item.id
                }
                schedulePinReset()
                await vm.pushHistoryEntryToServer(item)
            }

        case .file, .group:
            // reapplyHistoryItem copies the filename as text through the
            // observer (adopted write) — no raw UIPasteboard write here.
            vm.reapplyHistoryItem(item)
            withAnimation(.snappy) {
                pinnedItemId = item.id
            }
            schedulePinReset()
            Task { await vm.pushHistoryEntryToServer(item) }
        }
    }

    private func handleCopyFromOverlay(_ item: ClipboardHistoryItem) {
        switch item.entry.displayKind {
        case .text, .url:
            vm.reapplyHistoryItem(item)
            Task { await vm.pushHistoryEntryToServer(item) }
        case .image:
            if item.entry.hasData {
                Task {
                    await vm.applyAttachment(for: item)
                    guard vm.applyError == nil else { return }
                    vm.reapplyHistoryItem(item)
                    await vm.pushHistoryEntryToServer(item)
                }
            }
        case .file, .group:
            vm.reapplyHistoryItem(item)
            Task { await vm.pushHistoryEntryToServer(item) }
        }
    }

    private func schedulePinReset() {
        Task {
            try? await Task.sleep(for: .seconds(2))
            withAnimation(.snappy) {
                pinnedItemId = nil
            }
        }
    }

    private func handleSync() {
        vm.engine.forceTickNow()
    }

    @ViewBuilder
    private func contextMenuItems(for item: ClipboardHistoryItem) -> some View {
        Button {
            handleCopyFromOverlay(item)
        } label: {
            Label("复制", systemImage: "doc.on.doc")
        }

        switch item.entry.displayKind {
        case .text:
            Button {
                UIPasteboard.general.string = item.entry.text
            } label: {
                Label("复制为纯文本", systemImage: "doc.plaintext")
            }
        case .url:
            if let url = item.entry.parsedURL {
                Button {
                    UIApplication.shared.open(url)
                } label: {
                    Label("在浏览器中打开", systemImage: "safari")
                }
            }
            Button {
                UIPasteboard.general.string = item.entry.text
            } label: {
                Label("复制为纯文本", systemImage: "doc.plaintext")
            }
        case .image:
            Button {
                Task { await vm.saveAttachment(for: item) }
            } label: {
                Label("保存图片", systemImage: "square.and.arrow.down")
            }
        case .file, .group:
            Button {
                Task { await vm.saveAttachment(for: item) }
            } label: {
                Label("保存到 Documents", systemImage: "folder")
            }
        }

        shareLink(for: item)

        Divider()

        Button {
            isSelectMode = true
            selectedIds = [item.id]
        } label: {
            Label("选择", systemImage: "checkmark.circle")
        }

        Button(role: .destructive) {
            vm.removeHistoryItem(id: item.id)
        } label: {
            Label("删除", systemImage: "trash")
        }
    }

    /// Context-menu 分享 action as a native ShareLink. Picks the most
    /// useful payload per kind: URL for links, the loaded image (if cached)
    /// for images, the file name for files, otherwise the text.
    @ViewBuilder
    private func shareLink(for item: ClipboardHistoryItem) -> some View {
        let shareLabel = Label("分享", systemImage: "square.and.arrow.up")
        switch item.entry.displayKind {
        case .url:
            if let url = item.entry.parsedURL {
                ShareLink(item: url) { shareLabel }
            } else {
                ShareLink(item: item.entry.text) { shareLabel }
            }
        case .image:
            if let cached = thumbnailCache[item.id] {
                let image = Image(uiImage: cached)
                ShareLink(
                    item: image,
                    preview: SharePreview(item.entry.dataName ?? item.entry.text, image: image)
                ) { shareLabel }
            } else {
                ShareLink(item: item.entry.text) { shareLabel }
            }
        case .file, .group:
            ShareLink(item: item.entry.dataName ?? item.entry.text) { shareLabel }
        case .text:
            ShareLink(item: item.entry.text) { shareLabel }
        }
    }

    private func preloadThumbnails() {
        for item in vm.history {
            guard item.entry.type == .image,
                  item.entry.hasData,
                  thumbnailCache[item.id] == nil,
                  let hash = item.entry.hash, !hash.isEmpty
            else { continue }
            if let cached = ImageThumbnailCache.shared.cached(forHash: hash) {
                thumbnailCache[item.id] = cached
                continue
            }
            Task { await loadThumbnailIfNeeded(for: item) }
        }
    }

    private func loadThumbnailIfNeeded(for item: ClipboardHistoryItem) async {
        guard item.entry.type == .image,
              item.entry.hasData,
              thumbnailCache[item.id] == nil,
              let hash = item.entry.hash, !hash.isEmpty
        else { return }

        let thumb = await ImageThumbnailCache.shared.fetch(forHash: hash) {
            try await vm.fetchPreviewBytes(for: item)
        }
        if let thumb {
            thumbnailCache[item.id] = thumb
        }
    }

    private func loadURLMetadataIfNeeded(for item: ClipboardHistoryItem) async {
        guard item.entry.displayKind == .url,
              urlMetadataCache[item.id] == nil,
              let url = item.entry.parsedURL
        else { return }
        let metadata = await URLMetadataCache.shared.fetch(for: url)
        urlMetadataCache[item.id] = metadata
    }

    private func toggleSelection(_ id: UUID) {
        if selectedIds.contains(id) {
            selectedIds.remove(id)
        } else {
            selectedIds.insert(id)
        }
    }

    private func selectAll() {
        if selectedIds.count == displayedHistory.count {
            selectedIds = []
        } else {
            selectedIds = Set(displayedHistory.map(\.id))
        }
    }

    private func batchCopy() {
        let items = displayedHistory.filter { selectedIds.contains($0.id) }
        let texts = items.map(\.entry.text).joined(separator: "\n")
        UIPasteboard.general.string = texts
        exitSelectMode()
    }

    private func batchDelete() {
        for id in selectedIds {
            vm.removeHistoryItem(id: id)
        }
        exitSelectMode()
    }

    private func exitSelectMode() {
        isSelectMode = false
        selectedIds = []
    }

    private func openAppSettings() {
        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
        UIApplication.shared.open(url)
    }

    // MARK: - Error handling

    private var currentIssue: HomeIssue? {
        if let err = vm.saveError {
            return .saveFailed(message: errorMessage(err), detail: err.underlying)
        }
        switch engineState {
        case .authFailed:
            return .authFailed(detail: vm.engine.lastError?.underlying)
        case .offlineRetrying:
            if let err = vm.engine.lastError {
                return .offline(message: errorMessage(err), detail: err.underlying)
            }
            return .offline(message: String(localized: "离线 \u{00B7} 重试中"), detail: nil)
        case .loopDetected:
            return .loopDetected(detail: vm.engine.lastError?.underlying)
        case .idle, .succeeded, .hasNewUnwritten:
            return nil
        }
    }

    private func errorMessage(_ err: SyncError) -> String {
        switch err.kind {
        case .authFailed:                return String(localized: "认证失败 — 请检查用户名和密码")
        case .connectTimeout:            return String(localized: "连接超时 — 请检查服务器地址")
        case .receiveTimeout:            return String(localized: "接收超时 — 请稍后重试")
        case .networkUnreachable:        return String(localized: "无法连接 — 请检查网络和 URL")
        case .invalidURL:                return String(localized: "服务器地址无效")
        case .decodingFailed:            return String(localized: "服务器返回的数据无法解析")
        case .protocolError(let code):   return String(localized: "服务器返回 HTTP \(code)")
        case .serverError(let code):     return String(localized: "服务器错误 \(code)")
        case .notFound:                  return String(localized: "服务器尚未发布剪贴板")
        case .hashMismatch:              return String(localized: "内容校验失败 — 文件可能损坏")
        case .cancelled:                 return String(localized: "请求已取消")
        }
    }
}

// MARK: - Paste Permission Banner

/// Home hint nudging the user to set "从其他 App 粘贴" to "允许" once they've turned
/// on fully-automatic push -- otherwise iOS prompts "允许粘贴" on every engine
/// read. Dismissible; the choice persists via `pastePermissionHintDismissed`.
private struct PastePermissionBanner: View {
    var onOpenSettings: () -> Void
    var onDismiss: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "doc.on.clipboard")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.tint)
            VStack(alignment: .leading, spacing: 4) {
                Text("已开启自动推送")
                    .font(.footnote.weight(.semibold))
                Text("把「从其他 App 粘贴」设为「允许」可避免反复弹窗,让同步静默进行。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Button("打开设置", action: onOpenSettings)
                    .font(.caption.weight(.semibold))
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                    .padding(.top, 2)
            }
            Spacer(minLength: 8)
            Button(action: onDismiss) {
                Image(systemName: "xmark")
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("关闭")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.regularMaterial)
        .overlay(alignment: .bottom) {
            Divider().opacity(0.4)
        }
    }
}

// MARK: - Toolbar issue model + detail sheet

/// An error that's worth a red bang in the toolbar. Carries enough copy
/// to render both the icon's accessibility label and the detail sheet
/// without HomeView needing to re-derive strings.
private enum HomeIssue: Equatable {
    case authFailed(detail: String?)
    case offline(message: String, detail: String?)
    case saveFailed(message: String, detail: String?)
    case loopDetected(detail: String?)

    enum PrimaryAction {
        case goToSettings
        case retry
        case acknowledgeLoop
        case dismiss
    }

    var tint: Color {
        switch self {
        case .authFailed, .saveFailed:    .red
        case .offline, .loopDetected:     .orange
        }
    }

    var symbolName: String {
        switch self {
        case .authFailed:    "lock.fill"
        case .offline:       "wifi.exclamationmark"
        case .saveFailed:    "exclamationmark.triangle.fill"
        case .loopDetected:  "arrow.triangle.2.circlepath"
        }
    }

    var title: String {
        switch self {
        case .authFailed:    String(localized: "认证失败")
        case .offline:       String(localized: "无法连接服务器")
        case .saveFailed:    String(localized: "保存失败")
        case .loopDetected:  String(localized: "自动同步已暂停")
        }
    }

    var message: String {
        switch self {
        case .authFailed:
            String(localized: "请检查用户名和密码是否正确。修改后会自动重新同步。")
        case .offline(let message, _):
            message
        case .saveFailed(let message, _):
            message
        case .loopDetected:
            String(localized: "检测到同一份剪贴板内容在本机和服务器之间反复同步。点「重新启用」继续，或检查另一端的客户端是否也在自动同步同一份内容。")
        }
    }

    var detail: String? {
        switch self {
        case .authFailed(let d), .offline(_, let d), .saveFailed(_, let d), .loopDetected(let d):
            d
        }
    }

    var primaryAction: PrimaryAction {
        switch self {
        case .authFailed:    .goToSettings
        case .offline:       .retry
        case .saveFailed:    .dismiss
        case .loopDetected:  .acknowledgeLoop
        }
    }

    var primaryActionLabel: LocalizedStringKey {
        switch self {
        case .authFailed:    "去设置"
        case .offline:       "立即重试"
        case .saveFailed:    "好"
        case .loopDetected:  "重新启用"
        }
    }
}

private struct IssueDetailSheet: View {
    let issue: HomeIssue
    let onPrimaryAction: () -> Void

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: issue.symbolName)
                .font(.system(size: 44, weight: .regular))
                .foregroundStyle(issue.tint)
                .padding(.top, 24)

            VStack(spacing: 8) {
                Text(issue.title)
                    .font(.title3.weight(.semibold))
                Text(issue.message)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 24)
            }

            if let detail = issue.detail, !detail.isEmpty {
                ScrollView {
                    Text(detail)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                        .padding(12)
                }
                .frame(maxHeight: 80)
                .background(Color(.secondarySystemBackground),
                            in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .padding(.horizontal, 20)
            }

            Spacer(minLength: 0)

            VStack(spacing: 12) {
                Button(action: onPrimaryAction) {
                    Text(issue.primaryActionLabel)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.white)
                        .frame(height: 48)
                        .padding(.horizontal, 32)
                        .background(issue.tint, in: Capsule(style: .continuous))
                }

                Button {
                    dismiss()
                } label: {
                    Text("关闭")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 20)
        }
    }
}

// MARK: - Server Switcher Sheet

private struct ServerSwitcherSheet: View {
    @Bindable var vm: AppViewModel
    let onSelect: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var addDraft: ServerDraft?

    private var activeId: String? { vm.activeServer?.id }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(vm.servers.configs) { server in
                        Button {
                            onSelect(server.id)
                        } label: {
                            ServerSwitcherRow(
                                server: server,
                                isActive: server.id == activeId
                            )
                        }
                        .buttonStyle(.plain)
                        .listRowBackground(
                            server.id == activeId
                                ? Color.green.opacity(0.08)
                                : Color.clear
                        )
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("服务器")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark")
                            .font(.subheadline.weight(.semibold))
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        let existingNames = Set(vm.servers.configs.compactMap(\.name))
                        addDraft = ServerDraft(existingNames: existingNames)
                    } label: {
                        Image(systemName: "plus")
                    }
                }
            }
            .sheet(item: $addDraft) { draft in
                AddServerSheet(
                    draft: draft,
                    trustInsecureCert: Binding(
                        get: { vm.appSettings.trustInsecureCert },
                        set: { vm.appSettings.trustInsecureCert = $0 }
                    ),
                    ssidProvider: vm.ssidProvider,
                    onCancel: { addDraft = nil },
                    onSave: { saved in
                        commitDraft(saved)
                        addDraft = nil
                    }
                )
            }
        }
    }

    private func commitDraft(_ draft: ServerDraft) {
        let trimmedName = draft.name.trimmingCharacters(in: .whitespacesAndNewlines)
        let urls = draft.cleanedURLs
        guard !urls.isEmpty else { return }   // AddServerSheet's canSave gates this
        let server = ServerConfig(
            id: UUID().uuidString.lowercased(),
            name: trimmedName.isEmpty ? nil : trimmedName,
            urls: urls,
            username: draft.username,
            password: draft.password
        )
        var list = vm.servers
        list.configs.append(server)
        if list.activeConfigId == nil || list.configs.count == 1 {
            list.activeConfigId = server.id
        }
        vm.servers = list
    }
}

private struct ServerSwitcherRow: View {
    let server: ServerConfig
    let isActive: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: isActive ? "checkmark.circle.fill" : "circle")
                .font(.title3)
                .foregroundStyle(isActive ? Color.green : Color.secondary)
                .padding(.top, 2)

            VStack(alignment: .leading, spacing: 4) {
                Text(server.displayLabel)
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(.primary)
                Text(server.url)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                ServerURLClassSummary(urls: server.urls)
            }
            Spacer()
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// MARK: - Search Filter

private enum SearchDateFilter: Hashable {
    case all, today, yesterday, pastWeek

    var label: String {
        switch self {
        case .all:       String(localized: "全部")
        case .today:     String(localized: "今天")
        case .yesterday: String(localized: "昨天")
        case .pastWeek:  String(localized: "7 天内")
        }
    }

    func apply(to items: [ClipboardHistoryItem]) -> [ClipboardHistoryItem] {
        let cal = Calendar.current
        switch self {
        case .all:       return items
        case .today:     return items.filter { cal.isDateInToday($0.timestamp) }
        case .yesterday: return items.filter { cal.isDateInYesterday($0.timestamp) }
        case .pastWeek:
            guard let sevenAgo = cal.date(byAdding: .day, value: -7, to: .now) else { return items }
            return items.filter { $0.timestamp >= sevenAgo }
        }
    }

    static let allOptions: [SearchDateFilter] = [.all, .today, .yesterday, .pastWeek]
}

private struct SearchFilterSheet: View {
    @Binding var selectedTypes: Set<ClipboardDisplayKind>
    @Binding var selectedDate: SearchDateFilter

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("类型") {
                    ForEach(ClipboardDisplayKind.allCases, id: \.self) { kind in
                        Button {
                            if selectedTypes.contains(kind) {
                                selectedTypes.remove(kind)
                            } else {
                                selectedTypes.insert(kind)
                            }
                        } label: {
                            HStack {
                                Image(systemName: kind.symbolName)
                                    .foregroundStyle(kind.tint)
                                    .frame(width: 24)
                                Text(kind.localizedLabelString)
                                    .foregroundStyle(.primary)
                                Spacer()
                                if selectedTypes.contains(kind) {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(Color.accentColor)
                                }
                            }
                        }
                    }
                }

                Section("日期") {
                    ForEach(SearchDateFilter.allOptions, id: \.self) { option in
                        Button {
                            selectedDate = option
                        } label: {
                            HStack {
                                Text(option.label)
                                    .foregroundStyle(.primary)
                                Spacer()
                                if selectedDate == option {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(Color.accentColor)
                                }
                            }
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("筛选条件")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "checkmark")
                            .font(.subheadline.weight(.semibold))
                    }
                }
            }
        }
    }
}

// MARK: - Helpers

private extension Date {
    /// "刚刚" inside +/-5s, otherwise the system relative formatter.
    var relativeShort: String {
        let dt = timeIntervalSinceNow
        if abs(dt) < 5 { return String(localized: "刚刚") }
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f.localizedString(for: self, relativeTo: .now)
    }
}

private extension View {
    /// Applies the iOS 26 soft scroll-edge effect on top and bottom so the
    /// floating top/bottom bars get the system blur/fade under scrolling
    /// content. No-op below iOS 26.
    @ViewBuilder
    func scrollEdgeEffectStyleSoftTopBottom() -> some View {
        if #available(iOS 26.0, *) {
            self
                .scrollEdgeEffectStyle(.soft, for: .top)
                .scrollEdgeEffectStyle(.soft, for: .bottom)
        } else {
            self
        }
    }
}

private func formatSize(_ size: Int, kind: ClipboardDisplayKind) -> String {
    switch kind {
    case .text, .url:
        return String(localized: "\(size) 字")
    case .image, .file, .group:
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        return formatter.string(fromByteCount: Int64(size))
    }
}

// MARK: - Context Menu Preview

/// Custom preview shown by the native `.contextMenu(preview:)`. The system
/// animates the zoom-from-source transition; this view just supplies content.
/// Async-loaded images/long-text are fetched lazily in `.task`.
private struct CardPreviewView: View {
    let item: ClipboardHistoryItem
    @Bindable var vm: AppViewModel

    @State private var loadedImage: UIImage?
    @State private var loadedText: String?
    @State private var loadedOGImage: UIImage?
    @State private var loadedURLTitle: String?
    @State private var isLoading = false

    /// Fixed preview width. The targeted-preview platform sizes the platter to
    /// its content's intrinsic width — without a concrete width a short text or
    /// a small thumbnail collapses to ~80pt and, paired with a tall height,
    /// renders as a white pill (corner radius ≈ width/2). A fixed width keeps
    /// every preview reading as a card; 340 fits the narrowest iOS-17 device.
    private let previewWidth: CGFloat = 340

    var body: some View {
        Group {
            switch item.entry.displayKind {
            case .text:
                // A line-limited Text hugs short content and caps long content,
                // so the platter never stretches into a pill. (The full,
                // scrollable text lives in ClipboardPreviewSheet on tap.)
                Text(loadedText ?? item.entry.text)
                    .font(.body)
                    .lineLimit(18)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(16)

            case .url:
                VStack(alignment: .leading, spacing: 12) {
                    if let ogImage = loadedOGImage {
                        Image(uiImage: ogImage)
                            .resizable()
                            .scaledToFit()
                            .frame(maxWidth: .infinity)
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    } else if isLoading {
                        ProgressView()
                            .frame(maxWidth: .infinity, minHeight: 120)
                    }
                    if let title = loadedURLTitle, !title.isEmpty {
                        Text(title)
                            .font(.headline)
                    }
                    Text(item.entry.text)
                        .font(.callout)
                        .foregroundStyle(.blue)
                }
                .padding(16)

            case .image:
                Group {
                    if let image = loadedImage {
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFit()
                    } else if isLoading {
                        ProgressView()
                    } else {
                        Image(systemName: "photo.fill")
                            .font(.system(size: 48))
                            .foregroundStyle(.secondary)
                    }
                }
                // Reserve a box for the loading/placeholder states and cap a
                // tall image so the platter stays card-shaped, not column-tall.
                .frame(maxWidth: .infinity, minHeight: 220, maxHeight: 460)

            case .file, .group:
                VStack(spacing: 12) {
                    Image(systemName: item.entry.displayKind.symbolName)
                        .font(.system(size: 48))
                        .foregroundStyle(item.entry.displayKind.tint)
                    Text(item.entry.dataName ?? item.entry.text)
                        .font(.headline)
                        .lineLimit(2)
                        .multilineTextAlignment(.center)
                    if let size = item.entry.size {
                        let f = ByteCountFormatter()
                        Text(f.string(fromByteCount: Int64(size)))
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(24)
            }
        }
        // Fixed width (not a cap): a max-only width lets small content collapse
        // the platter into a pill. Height stays content-driven per case above.
        .frame(width: previewWidth)
        .task {
            await loadContent()
        }
    }

    private func loadContent() async {
        switch item.entry.displayKind {
        case .text where item.entry.hasData:
            isLoading = true
            if let bytes = try? await vm.fetchPreviewBytes(for: item) {
                loadedText = String(decoding: bytes, as: UTF8.self)
            }
            isLoading = false

        case .url:
            isLoading = true
            if let url = item.entry.parsedURL {
                let meta = await URLMetadataCache.shared.fetch(for: url)
                loadedOGImage = meta.ogImage
                loadedURLTitle = meta.title
            }
            isLoading = false

        case .image where item.entry.hasData:
            isLoading = true
            if let bytes = try? await vm.fetchPreviewBytes(for: item) {
                loadedImage = UIImage(data: bytes)
            }
            isLoading = false

        default:
            break
        }
    }
}

// MARK: - Preview

private func previewVM(history: [ClipboardHistoryItem]? = nil) -> AppViewModel {
    let vm = AppViewModel.preview()
    if let history { vm.history = history }
    return vm
}

#Preview("Home — 网格") {
    NavigationStack {
        HomeView(vm: previewVM())
    }
}

#Preview("Home — 空状态") {
    NavigationStack {
        HomeView(vm: previewVM(history: []))
    }
}
