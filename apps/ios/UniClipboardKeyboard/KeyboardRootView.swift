import SwiftUI
import UIKit

/// Single source of truth for the keyboard's vertical metrics. The
/// controller's height constraint is **computed** from these
/// (`contentHeight` / `stripBandHeight`), so a tweak here can no longer
/// drift out of sync with a hand-summed constant — exactly that drift
/// (top bar 36 → 38 without touching the controller's 252) once starved the
/// card row by 2pt, and the clipped card edges read as faint "divider lines"
/// above/below the cards and as the header covering them.
enum KeyboardLayout {
    /// iPad gets extra breathing room at the bottom: the input-switch /
    /// dismiss controls sit right under the keyboard frame there, and a
    /// flush key row makes them an easy mis-tap.
    static let isPad = UIDevice.current.userInterfaceIdiom == .pad

    static let topBarHeight: CGFloat = 38
    static let topBarVPad: CGFloat = 4

    /// Single horizontal grid for all four bands (top bar, card row, key
    /// row, globe strip). The bands used to sit on a 12/16/12/16 alternating
    /// inset, and the 4pt misregistration of their leading edges read as
    /// "header and cards are separate widgets".
    static let hMargin: CGFloat = 12

    static let cardHeight: CGFloat = 150
    static let cardRowVPad: CGFloat = 4

    static let keyRowHeight: CGFloat = 46
    static let keyRowTopPad: CGFloat = 4
    static let keyRowBottomPad: CGFloat = isPad ? 14 : 4

    static let globeSize: CGFloat = isPad ? 34 : 28
    static let stripHeight: CGFloat = isPad ? 34 : 30
    static let stripTopPad: CGFloat = isPad ? 6 : 2
    static let stripBottomPad: CGFloat = isPad ? 12 : 8

    /// Keyboard height without the globe strip. The middle band resolves to
    /// exactly `cardHeight + 2 × cardRowVPad`, so the card row is never
    /// clipped and never floats.
    static var contentHeight: CGFloat {
        topBarHeight + topBarVPad * 2
            + cardHeight + cardRowVPad * 2
            + keyRowTopPad + keyRowHeight + keyRowBottomPad
    }

    /// Extra band added when iOS wants an input-mode switch key.
    static var stripBandHeight: CGFloat {
        stripTopPad + stripHeight + stripBottomPad
    }
}

/// Solid cap/card surface — a shade lighter than the keyboard tray in both
/// schemes (white on light, mid-gray on dark). Shared by the key caps and
/// the clipboard cards so all opaque surfaces on the tray read as one family.
private let keyboardSurfaceColor = Color(uiColor: UIColor { trait in
    trait.userInterfaceStyle == .dark
        ? UIColor(white: 0.34, alpha: 1.0)
        : UIColor.white
})

/// The UniClip keyboard — a Paste-style hybrid: a top bar (🔍 + current
/// server), a horizontally-scrolling row of flat clipboard cards, and
/// a real key row (space / ⌫ / return) so light editing never forces a trip
/// back to the system keyboard. Tapping a card inserts its text inline
/// (downlink) or fetches + copies an image; a background pass auto-pushes
/// newly-copied device content (uplink). The 🌐 strip returns to a typing
/// keyboard.
struct KeyboardRootView: View {
    let model: KeyboardModel

    /// Search mode swaps the server-name bar for a type-scope filter strip
    /// (a custom keyboard has no QWERTY, so 🔍 filters rather than queries).
    @State private var searching = false
    @State private var filter: Filter = .all

    @State private var switchingServer = false
    @State private var serverChoices: [ServerConfig] = []
    @State private var activeServerId: String?

    enum Filter: Int, CaseIterable, Identifiable {
        case all, text, link, image
        var id: Int { rawValue }
        var title: LocalizedStringKey {
            switch self {
            case .all:   "全部"
            case .text:  "文本"
            case .link:  "链接"
            case .image: "图片"
            }
        }
        func matches(_ card: KeyboardModel.Card) -> Bool {
            switch self {
            case .all:   true
            case .text:  card.kind == .text
            case .link:  card.kind == .link
            case .image: card.kind == .image
            }
        }
    }

    var body: some View {
        ZStack(alignment: .top) {
            VStack(spacing: 0) {
                topBar
                    .padding(.horizontal, KeyboardLayout.hMargin)
                    // Symmetric, snug vertical insets so the bar hugs the card
                    // row beneath it — anything larger reads as a loose, empty
                    // band between the three stacked sections.
                    .padding(.vertical, KeyboardLayout.topBarVPad)
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                keyRow
                bottomStrip
            }

            // Server switcher rides above the whole keyboard so its scrim
            // catches taps anywhere — top bar, cards, keys — not just the
            // card area.
            if switchingServer {
                serverSwitcherOverlay
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // Deliberately NO opaque background: the system already draws the
        // keyboard tray (flat gray pre-iOS 26, Liquid Glass on iOS 26+) and it
        // auto-adapts to appearance + OS version. We keep the surface clear and
        // let that backdrop show through (the controller clears
        // UIHostingController's default opaque background). We also don't force
        // an appearance override — the UIKit tray follows the *system* scheme
        // and ignores preferredColorScheme, so following the system keeps our
        // content and the tray consistent.
        .tint(.accentColor)
    }

    // MARK: - Top bar

    @ViewBuilder
    private var topBar: some View {
        if searching {
            filterBar
        } else {
            serverBar
        }
    }

    private var serverBar: some View {
        ZStack {
            // Centered server name (tap → inline switcher). Bare text +
            // chevron — no resting surface (see the quiet-chrome note on
            // ChromePressStyle); the chevron carries the tap affordance.
            if model.gate == .ok {
                Button { openSwitcher() } label: {
                    HStack(spacing: 4) {
                        Text(verbatim: serverTitle)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        Image(systemName: "chevron.down")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(.secondary)
                            .rotationEffect(.degrees(switchingServer ? 180 : 0))
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 34)
                }
                .buttonStyle(ChromePressStyle(shape: Capsule(style: .continuous), hitOutset: 5))
                .accessibilityLabel(Text("切换服务器"))
            } else {
                Text(verbatim: "UniClip")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.primary)
            }

            HStack(spacing: 0) {
                if model.gate != .needsFullAccess, !model.cards.isEmpty {
                    circleButton(system: "magnifyingglass") {
                        withAnimation(.snappy(duration: 0.22)) { searching = true }
                    }
                    .accessibilityLabel(Text("筛选"))
                }
                Spacer(minLength: 0)
                if model.gate != .needsFullAccess {
                    syncButton
                        .animation(.snappy(duration: 0.28), value: model.isSyncing)
                        .animation(.snappy(duration: 0.28), value: model.syncFlash)
                }
            }
        }
        .frame(height: KeyboardLayout.topBarHeight)
    }

    /// Top-right control: spinner while syncing, a brief green ✓ / amber !
    /// right after a pass that moved data or failed, otherwise the refresh
    /// glyph. The outcome badge is the elegant replacement for the old
    /// "已发送本机内容…" text chip.
    @ViewBuilder
    private var syncButton: some View {
        if model.isSyncing {
            ProgressView()
                .controlSize(.small)
                .frame(width: 34, height: 34)
                .transition(.opacity)
        } else if let flash = model.syncFlash {
            Button {
                model.keyFeedback()
                model.refresh(force: true)
            } label: {
                Image(systemName: flash == .success ? "checkmark.circle.fill" : "exclamationmark.circle.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(flash == .success ? Color.green : Color.orange)
                    .frame(width: 34, height: 34)
            }
            .buttonStyle(ChromePressStyle(shape: Circle(), hitOutset: 5))
            .transition(.scale(scale: 0.6).combined(with: .opacity))
            .accessibilityLabel(Text(flash == .success ? "同步成功" : "同步失败"))
        } else {
            circleButton(system: "arrow.clockwise") {
                model.refresh(force: true)
            }
            .accessibilityLabel(Text("刷新"))
            .transition(.opacity)
        }
    }

    private var filterBar: some View {
        HStack(spacing: 8) {
            // 14pt: the ✕ sits beside footnote chip labels — the default 16
            // read heavier than the chips and pulled focus.
            circleButton(system: "xmark", size: 14) {
                withAnimation(.snappy(duration: 0.22)) {
                    searching = false
                    filter = .all
                }
            }
            .accessibilityLabel(Text("关闭筛选"))

            // Plain HStack, NOT a ScrollView: four fixed chips always fit
            // (even at 320pt width), and on iOS 26 a scrollable ScrollView
            // carries a scroll edge effect — a backdrop layer that the glass
            // tray renders as a translucent band across the whole viewport
            // (glass can't sample glass; same artifact family as glassEffect).
            HStack(spacing: 6) {
                ForEach(Filter.allCases) { f in
                    let isOn = f == filter
                    Button {
                        model.keyFeedback()
                        withAnimation(.snappy(duration: 0.2)) { filter = f }
                    } label: {
                        Text(f.title)
                            .font(.footnote.weight(.semibold))
                            // Selected chip is a tinted capsule, so the label
                            // is fixed white (accentColor resolves to system
                            // blue here — see returnKey). Unselected chips are
                            // bare chrome in primary, QuickType-style — the
                            // tray sits too close to .secondary for legibility.
                            .foregroundStyle(isOn ? .white : Color.primary)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .background {
                                if isOn {
                                    Capsule().fill(Color.accentColor)
                                }
                            }
                    }
                    .buttonStyle(ChromePressStyle(shape: Capsule(style: .continuous)))
                }
            }
            Spacer(minLength: 0)
        }
        .frame(height: KeyboardLayout.topBarHeight)
    }

    private var serverTitle: String {
        model.serverLabel.isEmpty ? String(localized: "UniClip") : model.serverLabel
    }

    // MARK: - Middle (cards / states + switcher overlay)

    private var displayedCards: [KeyboardModel.Card] {
        searching ? model.cards.filter(filter.matches) : model.cards
    }

    @ViewBuilder
    private var content: some View {
        switch model.gate {
        case .needsFullAccess:
            fullAccessHint
        case .noServer:
            cardArea
        case .ok:
            cardArea
        }
    }

    @ViewBuilder
    private var cardArea: some View {
        let cards = displayedCards
        if !cards.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                // Lazy so off-screen image cards don't all fire their
                // thumbnail fetch at once — bounds concurrent network + decode
                // work to what's near the viewport (keyboard memory).
                LazyHStack(spacing: 12) {
                    ForEach(cards) { card in
                        CardView(model: model, card: card)
                    }
                }
                .scrollTargetLayout()
                .padding(.vertical, KeyboardLayout.cardRowVPad)
            }
            // contentMargins (not padding inside the stack) keeps the resting
            // first card on the shared hMargin grid while still letting cards
            // bleed to the screen edge mid-scroll.
            .contentMargins(.horizontal, KeyboardLayout.hMargin, for: .scrollContent)
            .scrollTargetBehavior(.viewAligned)
            .keyboardScrollEdgeEffectHidden()
            .frame(maxHeight: .infinity)
        } else if model.isSyncing {
            centered {
                HStack(spacing: 8) {
                    ProgressView()
                    Text("正在同步…").font(.callout).foregroundStyle(.secondary)
                }
            }
        } else if let err = model.lastError {
            centered {
                VStack(spacing: 10) {
                    infoBlock(system: "exclamationmark.triangle", title: String(localized: "同步失败"), message: err)
                    Button {
                        model.keyFeedback()
                        model.refresh(force: true)
                    } label: {
                        Label("重试", systemImage: "arrow.clockwise")
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
        } else {
            centered {
                infoBlock(
                    system: searching ? emptyFilterIcon : "tray",
                    title: searching ? emptyFilterTitle : String(localized: "暂无剪贴板记录"),
                    message: String(localized: "复制文本或图片后回到这里即可发送")
                )
            }
        }
    }

    private var emptyFilterIcon: String {
        switch filter {
        case .all:   "tray"
        case .text:  "doc.text"
        case .link:  "link"
        case .image: "photo.on.rectangle"
        }
    }

    private var emptyFilterTitle: String {
        switch filter {
        case .all:   String(localized: "暂无剪贴板记录")
        case .text:  String(localized: "暂无文本记录")
        case .link:  String(localized: "暂无链接记录")
        case .image: String(localized: "暂无图片记录")
        }
    }

    // MARK: - Server switcher overlay

    private func openSwitcher() {
        let choices = model.serverChoices()
        serverChoices = choices.servers
        activeServerId = choices.activeId
        withAnimation(.snappy(duration: 0.22)) { switchingServer = true }
    }

    private func closeSwitcher() {
        withAnimation(.snappy(duration: 0.2)) { switchingServer = false }
    }

    private var serverSwitcherOverlay: some View {
        ZStack(alignment: .top) {
            // Full-surface tap-to-dismiss scrim: covers the top bar, cards and
            // key row, so a tap anywhere outside the panel closes the switcher.
            Color.black.opacity(0.001)
                .contentShape(Rectangle())
                .onTapGesture { closeSwitcher() }

            VStack(spacing: 2) {
                ForEach(serverChoices) { server in
                    Button {
                        model.keyFeedback()
                        if server.id != activeServerId { model.setActiveServer(server.id) }
                        closeSwitcher()
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: server.id == activeServerId ? "checkmark.circle.fill" : "circle")
                                .font(.body)
                                .foregroundStyle(server.id == activeServerId ? Color.green : Color.secondary)
                            Text(verbatim: server.displayLabel)
                                .font(.callout)
                                .foregroundStyle(.primary)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.vertical, 6)
            .frame(maxWidth: 260)
            // Opaque panel + shadow for elevation (no glass/material — see
            // flatSurface); the hairline keeps it separated in dark mode
            // where the shadow alone is too weak.
            .flatCard()
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .strokeBorder(Color.primary.opacity(0.08))
            )
            .shadow(color: .black.opacity(0.18), radius: 16, y: 6)
            // Drop below the top bar so it reads as a dropdown from the
            // server-name capsule rather than overlapping it.
            .padding(.top, 50)
            .transition(.move(edge: .top).combined(with: .opacity))
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    // MARK: - Key row

    private var keyRow: some View {
        HStack(spacing: 7) {
            spaceKey.frame(maxWidth: .infinity)
            HStack(spacing: 7) {
                BackspaceKey(model: model).frame(maxWidth: .infinity)
                returnKey.frame(maxWidth: .infinity)
            }
            .frame(maxWidth: .infinity)
        }
        .frame(height: KeyboardLayout.keyRowHeight)
        .padding(.horizontal, KeyboardLayout.hMargin)
        .padding(.top, KeyboardLayout.keyRowTopPad)
        // Breathing room below the keys. On iPad the system parks its
        // input-switch / dismiss controls right under the keyboard frame, and
        // a flush key row made them an easy mis-tap — hence the wider inset
        // there. On iPhone a slim gap to the globe strip / bottom edge is
        // enough.
        .padding(.bottom, KeyboardLayout.keyRowBottomPad)
    }

    private var spaceKey: some View {
        Button {
            model.keyFeedback()
            model.insertText(" ")
        } label: {
            // Labeled like the system space bar — a blank white cap next to
            // the labeled 发送 key read as a broken placeholder.
            Text("空格")
                .font(.callout)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .flatKey()
                .contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    private var returnKey: some View {
        Button {
            model.keyFeedback()
            model.insertText("\n")
        } label: {
            Group {
                if let title = model.returnKeyTitle {
                    Text(title).font(.callout.weight(.semibold))
                } else {
                    Image(systemName: "return").font(.system(size: 18, weight: .medium))
                }
            }
            // Return is an emphasized solid key. The cap is Color.accentColor,
            // which in THIS extension resolves to the system blue: the appex
            // bundle doesn't carry the app's AccentColor asset (it lives under
            // UniClipboard/, which the keyboard target doesn't compile), so the
            // app's dark-ink / ivory pair never applies here. A fixed white
            // glyph reads cleanly on that blue in both schemes — the old
            // systemBackground glyph went black in dark mode (black-on-blue).
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("回车"))
    }

    // MARK: - Bottom strip (globe / dismiss)

    @ViewBuilder
    private var bottomStrip: some View {
        // Only the globe lives down here. When iOS doesn't need a keyboard
        // switch key (e.g. UniClip is the only third-party keyboard) the
        // whole strip collapses so the keyboard ends right under the key row
        // instead of leaving a tall empty band above the home indicator.
        if model.needsInputModeSwitchKey {
            HStack(spacing: 0) {
                glyphButton(system: "globe", size: KeyboardLayout.globeSize) { model.advanceInputMode() }
                    .accessibilityLabel(Text("切换键盘"))
                Spacer(minLength: 0)
            }
            .padding(.horizontal, KeyboardLayout.hMargin)
            // The fixed frame comes FIRST, paddings after — the old order
            // (paddings inside a 30pt frame) silently overflowed the frame by
            // ±4pt, letting the globe creep up toward the key row.
            .frame(height: KeyboardLayout.stripHeight)
            .padding(.top, KeyboardLayout.stripTopPad)
            // Keep a little breathing room above the home indicator — flush
            // against the bottom edge reads as cramped.
            .padding(.bottom, KeyboardLayout.stripBottomPad)
        }
    }

    // MARK: - Full-access hint

    private var fullAccessHint: some View {
        centered {
            VStack(spacing: 12) {
                Image(systemName: "lock.shield")
                    .font(.largeTitle)
                    .foregroundStyle(.tint)
                Text("需要「完全访问权限」")
                    .font(.headline)
                Text("在 设置 › 通用 › 键盘 › UniClip 中开启「允许完全访问」,即可在打开键盘时自动同步剪贴板。")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 24)
                Button("前往设置 ›") {
                    model.openSettings()
                }
                .buttonStyle(.borderedProminent)
                .foregroundStyle(Color(.systemBackground))
            }
        }
    }

    // MARK: - Small building blocks

    private func infoBlock(system: String, title: String, message: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: system)
                .font(.title2)
                .foregroundStyle(.secondary)
            Text(title)
                .font(.callout.weight(.medium))
            Text(message)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 24)
        }
    }

    private func glyphButton(system: String, size: CGFloat, action: @escaping () -> Void) -> some View {
        Button {
            model.keyFeedback()
            action()
        } label: {
            Image(systemName: system)
                .font(.system(size: KeyboardLayout.isPad ? 19 : 16, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: size, height: size)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    /// Bare circular button for the top bar — same chrome grammar as the
    /// globe key in the bottom strip (secondary glyph, no resting surface,
    /// highlight only while pressed), so 🔍 / ⟳ / ✕ read as tray chrome
    /// rather than competing with the card surfaces below.
    private func circleButton(system: String, size: CGFloat = 16, action: @escaping () -> Void) -> some View {
        Button {
            model.keyFeedback()
            action()
        } label: {
            Image(systemName: system)
                .font(.system(size: size, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: 34, height: 34)
        }
        .buttonStyle(ChromePressStyle(shape: Circle(), hitOutset: 5))
    }

    private func centered<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        VStack {
            Spacer(minLength: 0)
            content()
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Backspace key (hold-to-repeat)

/// Backspace with press-and-hold auto-repeat: one delete on touch-down, then
/// after a short hold it repeats on an accelerating interval (like the system
/// keyboard). Uses `onLongPressGesture`'s `onPressingChanged` to bracket the
/// press; the repeat loop is a cancellable `Task` torn down on release /
/// disappear, with an iteration cap as a runaway backstop.
private struct BackspaceKey: View {
    let model: KeyboardModel

    @State private var pressing = false
    @State private var repeatTask: Task<Void, Never>?

    var body: some View {
        Image(systemName: "delete.left")
            .font(.system(size: 18, weight: .medium))
            .foregroundStyle(.primary)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .flatKey()
            .contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
            .opacity(pressing ? 0.55 : 1)
            .animation(.easeOut(duration: 0.08), value: pressing)
            // minimumDuration 0 ⇒ onPressingChanged(true) on touch-down; the
            // large maximumDistance keeps a slight finger slide from cancelling.
            .onLongPressGesture(minimumDuration: 0, maximumDistance: 1000) {
                // perform — no-op; the press lifecycle is handled below.
            } onPressingChanged: { isPressing in
                pressing = isPressing
                if isPressing { startRepeating() } else { stopRepeating() }
            }
            .onDisappear { stopRepeating() }
            .accessibilityLabel(Text("删除"))
            .accessibilityAddTraits(.isButton)
    }

    private func startRepeating() {
        repeatTask?.cancel()
        repeatTask = Task { @MainActor in
            model.keyFeedback()                           // tactile confirm on touch-down
            model.deleteBackward()                        // immediate first delete
            try? await Task.sleep(for: .seconds(0.45))    // hold before auto-repeat kicks in
            var interval: Double = 0.11
            var count = 0
            while !Task.isCancelled, count < 600 {        // cap: runaway backstop
                model.keyFeedback(haptic: false)          // click on each repeat, no buzz
                model.deleteBackward()
                count += 1
                try? await Task.sleep(for: .seconds(interval))
                interval = max(0.035, interval * 0.90)    // accelerate toward ~28/s
            }
        }
    }

    private func stopRepeating() {
        repeatTask?.cancel()
        repeatTask = nil
    }
}

// MARK: - Card

private struct CardView: View {
    let model: KeyboardModel
    let card: KeyboardModel.Card

    private var isActing: Bool { model.actingCardID == card.id }
    private var didAct: Bool { model.actedCardID == card.id }

    var body: some View {
        Button {
            model.activate(card)
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                header
                content
            }
            .padding(12)
            .frame(width: 152, height: KeyboardLayout.cardHeight, alignment: .topLeading)
            // Deliberately NOT Liquid Glass: cards are *content* surfaces
            // sitting directly on the system keyboard tray, which is itself
            // glass on iOS 26+ — and glass cannot sample other glass, so
            // stacking them rendered noisy edges that read as faint divider
            // lines. A solid cap-colored fill keeps the card row and the key
            // caps reading as one continuous family on the tray.
            .flatCard()
            .overlay(alignment: .center) {
                if didAct { actedOverlay }
            }
            // The card IS the button: without an explicit content shape, a
            // .plain button only hit-tests its rendered subviews (the text
            // glyphs), so the empty area below short text and the image
            // padding felt dead. Pin the hit target to the whole card.
            .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(model.actingCardID != nil)
        .animation(.easeInOut(duration: 0.2), value: didAct)
    }

    // "Text  3m" left, activity spinner right — mirrors Paste's card header.
    private var header: some View {
        HStack(spacing: 6) {
            Image(systemName: kindIcon)
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(kindTint)
            Text(kindLabel)
                .font(.caption.weight(.semibold))
                .foregroundStyle(kindTint)
            Text(card.time)
                .font(.caption2)
                .foregroundStyle(.tertiary)
            Spacer(minLength: 4)
            if isActing { ProgressView().controlSize(.mini) }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch card.kind {
        case .text:
            Text(card.title)
                .font(.callout)
                .foregroundStyle(.primary)
                .lineLimit(5)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        case .link:
            VStack(alignment: .leading, spacing: 6) {
                Text(card.title)
                    .font(.callout)
                    .foregroundStyle(.primary)
                    .lineLimit(3)
                    .multilineTextAlignment(.leading)
                if let host = card.subtitle {
                    HStack(spacing: 4) {
                        Image(systemName: "link").font(.system(size: 10, weight: .semibold))
                        Text(host).font(.caption2).lineLimit(1).truncationMode(.middle)
                    }
                    .foregroundStyle(.tint)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        case .image:
            CardThumbnail(model: model, card: card)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var actedOverlay: some View {
        ZStack {
            // Flat frost, not material — the keyboard avoids backdrop-effect
            // surfaces entirely (see the flat-surface helpers note).
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(keyboardSurfaceColor.opacity(0.92))
            Label(actedLabel, systemImage: "checkmark.circle.fill")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.green)
                .labelStyle(.titleAndIcon)
        }
        .transition(.opacity)
    }

    private var kindIcon: String {
        switch card.kind {
        case .text:  "text.alignleft"
        case .link:  "link"
        case .image: "photo"
        }
    }

    private var kindLabel: LocalizedStringKey {
        switch card.kind {
        case .text:  "文本"
        case .link:  "链接"
        case .image: "图片"
        }
    }

    private var kindTint: Color {
        switch card.kind {
        case .text:  .secondary
        case .link:  .accentColor
        case .image: .orange
        }
    }

    private var actedLabel: LocalizedStringKey {
        card.kind == .image ? "已复制" : "已插入"
    }
}

// MARK: - Lazy image thumbnail

/// Image card body: a rounded placeholder that fills in with a downsampled
/// thumbnail once `KeyboardModel.thumbnail(for:)` resolves. Keyed on the card
/// id so reused slots reload; failures / oversize originals leave the
/// placeholder in place.
private struct CardThumbnail: View {
    let model: KeyboardModel
    let card: KeyboardModel.Card

    @State private var image: UIImage?
    @State private var didLoad = false

    private let shape = RoundedRectangle(cornerRadius: 10, style: .continuous)

    var body: some View {
        // The gradient fill is the PRIMARY view, so it alone defines the
        // layout size (a Shape fills exactly the proposed slot). The thumbnail
        // rides as an `.overlay`: overlays never resize their primary, so
        // `scaledToFill`'s deliberately-oversized measurement can't grow the
        // card the way it did inside a ZStack (a flexible `.frame` won't clamp
        // it either — `max:.infinity` lets the oversize through). `clipShape`
        // then trims the overflow to a centered crop.
        shape
            .fill(
                LinearGradient(
                    colors: [Color.orange.opacity(0.18), Color.pink.opacity(0.12)],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )
            )
            .overlay {
                if let image {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                        .transition(.opacity)
                } else {
                    Image(systemName: didLoad ? "photo" : "photo.badge.arrow.down")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                        .symbolEffect(.pulse, isActive: !didLoad)
                }
            }
            .clipShape(shape)
            .task(id: card.id) {
                didLoad = false
                image = await model.thumbnail(for: card)
                didLoad = true
            }
            .animation(.easeInOut(duration: 0.2), value: image)
    }
}

// MARK: - Chrome press style

/// Quiet-chrome press feedback. The keyboard's chrome (top bar, globe strip)
/// carries **no resting surface** — bare glyphs and text sit directly on the
/// system tray, exactly like the system keyboard's QuickType bar and globe
/// key. Surfaces are reserved for *content* (cards) and *keys* (caps); giving
/// chrome its own opaque islands split the header off from the card row as a
/// second, sparser surface band. A soft highlight that exists only while
/// pressed keeps the tap affordance without re-introducing that band.
private struct ChromePressStyle<S: InsettableShape>: ButtonStyle {
    let shape: S

    /// Extra hit-test halo beyond the visual bounds. Top-bar controls draw at
    /// 34pt for visual rhythm, but HIG wants ≥44pt targets — and with no
    /// resting surface the eye aims at the ~14pt glyph, so finger scatter is
    /// wider than it was against a filled circle. Only the hit shape grows;
    /// the press highlight stays at the drawn size. The top-bar band is 46pt
    /// tall (4+38+4), so a 44pt hit circle still lands inside the keyboard.
    var hitOutset: CGFloat = 0

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(shape.fill(Color.primary.opacity(configuration.isPressed ? 0.08 : 0)))
            .contentShape(shape.inset(by: -hitOutset))
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

// MARK: - Flat surface helpers

/// The keyboard deliberately carries **no Liquid Glass and no material
/// surfaces of its own**. The tray behind us (`UIInputView.Style.keyboard`)
/// is already a system blur — Liquid Glass on iOS 26+ — and glass cannot
/// sample other glass: `glassEffect` elements hosted here rendered a
/// translucent backdrop band around the top bar plus a hairline where the
/// band ended, which read as "the header occludes the cards". The surfaces
/// we do draw — cards, key caps, the switcher panel — are flat opaque fills,
/// the same treatment the system keyboard gives its own key caps inside the
/// blurred tray; chrome (top bar, globe strip) draws no resting surface at
/// all (see ChromePressStyle).
private extension View {
    /// Flat, opaque surface in the given shape — the keyboard's only
    /// surface treatment (white on light, mid-gray on dark).
    func flatSurface<S: Shape>(in shape: S) -> some View {
        background(keyboardSurfaceColor, in: shape)
    }

    /// Key cap for the space / ⌫ keys.
    func flatKey() -> some View {
        flatSurface(in: RoundedRectangle(cornerRadius: 9, style: .continuous))
    }

    /// Clipboard-card / panel surface — same fill, larger radius, so cards
    /// and key caps read as one continuous family on the tray.
    func flatCard() -> some View {
        flatSurface(in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }

    /// iOS 26 attaches a scroll edge effect — a backdrop layer — to
    /// scrollable ScrollViews. Hosted on the keyboard's glass tray that
    /// backdrop degenerates into the translucent-band artifact (glass can't
    /// sample glass — same family as glassEffect), so the keyboard hides it
    /// on every ScrollView it keeps.
    @ViewBuilder
    func keyboardScrollEdgeEffectHidden() -> some View {
        if #available(iOS 26, *) {
            scrollEdgeEffectHidden(true)
        } else {
            self
        }
    }
}

// MARK: - Preview

#if DEBUG
#Preview("Keyboard — 卡片流") {
    KeyboardRootView(model: .previewReady())
        .frame(height: KeyboardLayout.contentHeight + KeyboardLayout.stripBandHeight)
        .background(Color(.systemGray5))
}

#Preview("Keyboard — 空状态") {
    KeyboardRootView(model: .previewEmpty())
        .frame(height: KeyboardLayout.contentHeight + KeyboardLayout.stripBandHeight)
        .background(Color(.systemGray5))
}
#endif
