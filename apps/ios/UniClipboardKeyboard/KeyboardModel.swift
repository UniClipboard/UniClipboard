import Foundation
import UIKit
import ImageIO
import Network
import Observation
import OSLog

private let log = Logger(subsystem: "app.uniclipboard.keyboard", category: "sync")

/// Observable state + sync logic backing the UniClip keyboard. Owned by
/// `KeyboardViewController`; the SwiftUI `KeyboardRootView` reads its
/// published properties and calls its actions.
///
/// The screen is a compact clipboard-history browser, not a QWERTY: a
/// horizontally-scrolling row of cards distilled from the App Group history
/// log (`SettingsStore.loadHistory()`), filterable by 最近 / 文本 / 图片.
/// Tapping a card inserts its text inline (uplink-free) or fetches + copies
/// an image to the pasteboard. A background sync pass pushes anything newly
/// copied on the device (uplink) and pulls the server's latest entry
/// (downlink) so the row stays live.
///
/// MainActor-isolated (the target's default isolation). Pasteboard reads run
/// on main; network work hops off via `await` on the non-isolated
/// `SyncClipboardClient`.
@MainActor
@Observable
final class KeyboardModel {

    // MARK: - Top-level gate

    /// What the content area should render *before* we even look at cards:
    /// the two hard prerequisites (Full Access, a configured server) win over
    /// any history we might have cached.
    enum Gate: Equatable {
        case ok
        case needsFullAccess
        case noServer
    }

    /// Result of the uplink half of a sync pass. No longer shown as text —
    /// kept so a pass can tell whether it actually pushed (drives `syncFlash`).
    enum PushStatus: Equatable {
        case none                 // nothing on the device pasteboard
        case skipped              // present, but already synced (== watermark)
        case pushed(String)       // pushed; payload is a short summary
        case failed(String)
    }

    /// Transient sync-outcome badge shown *on the refresh button*: a brief
    /// green ✓ after a pass that actually moved data, a brief amber ! after a
    /// failed pull. Replaces the old verbose "已发送本机内容…" status text.
    enum SyncFlash: Equatable { case success, failure }

    /// One card in the horizontal row — a `ClipboardHistoryItem` distilled
    /// into display-ready fields. Built from history *metadata*: text cards
    /// carry their value inline (ready to insert), image cards defer both the
    /// thumbnail and the full-payload fetch to lazy network calls so a row of
    /// cards never pulls multi-MB blobs into the keyboard's tight memory
    /// budget up front. The underlying `entry` is retained for the tap action
    /// and the thumbnail fetch.
    struct Card: Identifiable, Equatable {
        enum Kind: Equatable { case text, link, image }

        let id: UUID            // the history item's stable id
        let kind: Kind
        let entry: Clipboard    // underlying snapshot — drives action + thumbnail
        let title: String       // text snippet / "图片"
        let subtitle: String?   // URL host for links, else nil
        let time: String        // relative-short timestamp ("9:41" style)
        let sizeText: String?   // "128 字" / "1.2 MB"

        /// Tabs this card belongs to. `链接` rides in the 文本 tab.
        var isText: Bool { kind == .text || kind == .link }
        var isImage: Bool { kind == .image }
    }

    // MARK: - Published state

    var hasFullAccess: Bool = false
    var needsInputModeSwitchKey: Bool = true

    /// Key-feedback prefs, mirrored from `AppSettings` (App Group). Read
    /// once on appear and re-read on each sync pass so a change made in the
    /// main app takes effect the next time the keyboard opens. Default true
    /// so a fresh install feels like a stock keyboard.
    private(set) var soundFeedback = true
    private(set) var hapticFeedback = true

    private(set) var gate: Gate = .ok
    /// Drives the header's refresh spinner. Independent of `cards` so a sync
    /// pass never blanks out the already-visible row.
    private(set) var isSyncing: Bool = false
    /// Set on a failed pull / tap-fetch. Rendered as an inline chip (cards
    /// present) or a full hint + retry (no cards).
    private(set) var lastError: String?
    private(set) var cards: [Card] = []
    private(set) var pushStatus: PushStatus = .none
    /// The entry the most recent uplink actually uploaded. Read by the
    /// downlink half to decide whether the server's latest is our own push
    /// (→ adopt its hash as watermark) or someone else's (→ treat as pull).
    private var lastPushedEntry: Clipboard?
    /// Brief success/failure badge on the refresh button; auto-clears.
    private(set) var syncFlash: SyncFlash?
    private(set) var serverLabel: String = ""

    /// The card whose deferred payload (long text / image) is being fetched,
    /// so just that card can show a spinner.
    private(set) var actingCardID: UUID?
    /// Briefly set right after an insert/copy so the tapped card can flash a
    /// "已插入 / 已复制" confirmation without a separate state machine.
    private(set) var actedCardID: UUID?

    /// Context-appropriate label for the Return key, derived from the host
    /// field's `returnKeyType` (发送 / 搜索 / …). `nil` ⇒ render the ↵ glyph.
    /// Set by the controller; a custom keyboard can read the type but can
    /// only ever *insert a newline*, which most single-line fields submit on.
    private(set) var returnKeyTitle: String?

    /// Server + trust resolved on the last sync pass, reused by a card tap to
    /// fetch its deferred payload / thumbnail without re-reading the store.
    private var ctx: (server: ServerConfig, trust: Bool)?

    // MARK: - UI callbacks (wired by the controller)

    var insertText: (String) -> Void = { _ in }
    var deleteBackward: () -> Void = {}
    var advanceInputMode: () -> Void = {}
    var dismiss: () -> Void = {}
    /// Plays the system key-click sound. Wired by the controller to
    /// `UIDevice.current.playInputClick()` — which only fires when the
    /// input view adopts `UIInputViewAudioFeedback` AND the user has
    /// 键盘点击音 enabled, so the model never has to check that itself.
    var playInputClick: () -> Void = {}
    var openSettings: () -> Void = {}

    /// Reused light-impact generator for key haptics. Kept warm via
    /// `prepare()` so a press fires with minimal latency.
    private let impactGenerator = UIImpactFeedbackGenerator(style: .light)

    /// One App-Group store for the keyboard's lifetime — reused by the live
    /// poll (~1.2s) and the sync paths so we don't re-run the store's
    /// init-time migrations on every tick.
    private let store = SettingsStore()

    /// Decoded thumbnails keyed by image content hash. Bounded by NSCache's
    /// own eviction so a long-lived keyboard session can't grow unbounded.
    private let thumbnailCache = NSCache<NSString, UIImage>()

    /// Monotonic token: only the *latest* sync pass is allowed to publish
    /// state. A fast re-appear / manual ⟳ bumps this so a stale in-flight
    /// pass that resumes after cancellation can't clobber fresh state (e.g.
    /// flip `isSyncing` back off after a newer pass turned it on).
    private var syncGeneration = 0
    private var syncTask: Task<Void, Never>?
    private var flashTask: Task<Void, Never>?
    /// Polls `UIPasteboard.changeCount` while the keyboard is on screen so a
    /// copy made *with the keyboard already open* auto-syncs without a manual
    /// refresh tap. Reading `changeCount` is free and never prompts.
    private var pollTask: Task<Void, Never>?

    /// Live network-path facts for §5.3 auto-switch, maintained by
    /// `pathMonitor`. `NWPathMonitor` needs no entitlement (unlike SSID), so
    /// the keyboard reads its own interface type; only the SSID *name* comes
    /// from the App Group.
    private var pathIsWifi = false
    private var pathIsCellular = false
    private var pathIsTailscale = false
    private var pathMonitorStarted = false
    private let pathMonitor = NWPathMonitor()
    private let pathQueue = DispatchQueue(label: "app.uniclipboard.keyboard.path", qos: .utility)

    // MARK: - Lifecycle

    /// Called from `viewDidAppear`. Gates on Full Access, shows cached
    /// history instantly, runs an initial sync pass, and starts watching the
    /// pasteboard for changes while open.
    func onAppear() {
        // Load feedback prefs first — the space/⌫/return keys work (and so
        // should honor the click/haptic toggles) even before Full Access,
        // i.e. before the gate below short-circuits.
        loadFeedbackPrefs()
        impactGenerator.prepare()
        guard hasFullAccess else {
            gate = .needsFullAccess
            return
        }
        reloadCards()        // instant, offline — render before the network round-trip
        startPathMonitoring()
        refresh()
        startMonitoring()
    }

    /// Mirror the keyboard-feedback toggles out of the App Group settings.
    /// Cheap (one `UserDefaults` data decode); called on appear and on each
    /// sync pass so a change in the main app is picked up promptly.
    private func loadFeedbackPrefs() {
        let s = store.loadAppSettings()
        soundFeedback = s.keyboardSoundFeedback
        hapticFeedback = s.keyboardHapticFeedback
    }

    /// Fire key feedback for a button/key tap: the system click sound and a
    /// light haptic, each gated by the user's prefs. `haptic: false` suppresses
    /// only the haptic (used by backspace auto-repeat, where a buzz on every
    /// repeat tick would be unpleasant while the click still reads as typing).
    func keyFeedback(haptic: Bool = true) {
        if soundFeedback { playInputClick() }
        if haptic, hapticFeedback {
            impactGenerator.impactOccurred()
            impactGenerator.prepare()   // re-arm for the next press
        }
    }

    /// Re-run the sync pass. Cancels any in-flight pass first so a fast
    /// re-appear (or a manual ⟳ tap) doesn't race two pulls.
    ///
    /// `force` bypasses the changeCount gate in the uplink — used by the
    /// manual refresh button so the user can retry a failed push (or re-pull)
    /// even when nothing new has been copied. Automatic triggers (appear,
    /// poll) leave it false so reopening the keyboard never re-prompts.
    func refresh(force: Bool = false) {
        guard hasFullAccess else {
            gate = .needsFullAccess
            return
        }
        syncGeneration += 1
        let gen = syncGeneration
        syncTask?.cancel()
        syncTask = Task { [weak self] in
            await self?.sync(force: force, gen: gen)
        }
    }

    /// Begin polling the pasteboard `changeCount` (~1.2s) while the keyboard
    /// is visible. When it advances past what we last synced — i.e. the user
    /// copied something new with the keyboard already up — fire an automatic
    /// sync. Idempotent; `stopMonitoring()` tears it down on disappear.
    func startMonitoring() {
        guard hasFullAccess else { return }
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                if Task.isCancelled { return }
                self?.pollTick()
            }
        }
    }

    func stopMonitoring() {
        pollTask?.cancel()
        pollTask = nil
    }

    deinit { pathMonitor.cancel() }

    /// Begin watching the network path (Wi-Fi / cellular / other). Needs no
    /// entitlement — `NWPathMonitor` is free — so the keyboard reads interface
    /// type itself; only the SSID *name* comes from the App Group. Started
    /// once (the monitor can't restart after cancel) and torn down in
    /// `deinit`. A change re-runs the sync so the §5.3 effective server
    /// follows the network.
    private var pathInitialized = false

    private func startPathMonitoring() {
        guard !pathMonitorStarted else { return }
        pathMonitorStarted = true
        pathMonitor.pathUpdateHandler = { [weak self] path in
            let wifi = path.usesInterfaceType(.wifi)
            let cellular = path.usesInterfaceType(.cellular)
            let tailscale = TailscaleDetector.isActive()
            Task { @MainActor [weak self] in
                guard let self else { return }
                let changed = self.pathIsWifi != wifi
                    || self.pathIsCellular != cellular
                    || self.pathIsTailscale != tailscale
                self.pathIsWifi = wifi
                self.pathIsCellular = cellular
                self.pathIsTailscale = tailscale
                guard self.pathInitialized else {
                    self.pathInitialized = true
                    return
                }
                if changed, self.hasFullAccess { self.refresh() }
            }
        }
        pathMonitor.start(queue: pathQueue)
    }

    /// The current §5.3 `NetworkContext`. Interface type comes from our own
    /// `NWPathMonitor`; the SSID name from the App Group (the main app writes
    /// it). On cellular we deliberately drop any `last_known_ssid` — there's
    /// no Wi-Fi, and trusting a stale name would wrongly keep a Wi-Fi rule
    /// active. This is what lets the keyboard follow a Wi-Fi→cellular switch
    /// even when the main app hasn't run to clear the stored SSID.
    private func currentNetworkContext() -> NetworkContext {
        // Tailscale checked live — getifaddrs is cheap and needs no
        // entitlement, so the keyboard follows Tailscale on its own.
        let tailscale = TailscaleDetector.isActive()
        if pathIsWifi {
            return NetworkContext(
                ssid: store.loadLastKnownSSID(),
                isWifi: true,
                isCellular: false,
                isTailscale: tailscale
            )
        }
        if pathIsCellular {
            return NetworkContext(ssid: nil, isWifi: false, isCellular: true, isTailscale: tailscale)
        }
        return NetworkContext(ssid: nil, isWifi: false, isCellular: false, isTailscale: tailscale)
    }

    /// One poll iteration: if a sync isn't already running and the pasteboard
    /// advanced past our last sync, kick off an automatic pass. `changeCount`
    /// is free — only a genuine new copy triggers the (possibly-prompting)
    /// content read inside the uplink.
    private func pollTick() {
        guard hasFullAccess, gate == .ok, !isSyncing else { return }
        let cc = UIPasteboard.general.changeCount
        if cc != store.loadLastSyncedChangeCount() {
            refresh()
        }
    }

    // MARK: - Server switching

    /// Snapshot of the configured servers for the inline switcher overlay.
    /// Read on demand (the store isn't `@Observable`); the overlay captures
    /// the result when it opens.
    func serverChoices() -> (servers: [ServerConfig], activeId: String?) {
        let list = store.loadServers()
        return (list.configs, list.activeConfig?.id)
    }

    /// Make `id` the active server (writes `activeConfigId` to the App Group,
    /// same as the app's `setActiveServer`) and re-sync against it. The app
    /// picks the change up on its next foreground read.
    func setActiveServer(_ id: String) {
        var list = store.loadServers()
        guard list.activeConfigId != id, list.configs.contains(where: { $0.id == id }) else { return }
        list.activeConfigId = id
        store.saveServers(list)
        serverLabel = list.activeConfig?.displayLabel ?? ""
        refresh(force: true)
    }

    // MARK: - Return key

    /// Record the host field's Return-key intent so the key can label itself
    /// (发送 / 搜索 / …) like the system keyboard. Called by the controller on
    /// appear / when the input context changes.
    func setReturnKeyType(_ type: UIReturnKeyType?) {
        switch type ?? .default {
        case .go:                       returnKeyTitle = String(localized: "前往")
        case .search, .google, .yahoo:  returnKeyTitle = String(localized: "搜索")
        case .send:                     returnKeyTitle = String(localized: "发送")
        case .done:                     returnKeyTitle = String(localized: "完成")
        case .next:                     returnKeyTitle = String(localized: "下一项")
        case .continue:                 returnKeyTitle = String(localized: "继续")
        case .join:                     returnKeyTitle = String(localized: "加入")
        default:                        returnKeyTitle = nil   // .default → ↵ glyph
        }
    }

    // MARK: - Sync

    private func sync(force: Bool, gen: Int) async {
        let servers = store.loadServers()
        let settings = store.loadAppSettings()
        soundFeedback = settings.keyboardSoundFeedback
        hapticFeedback = settings.keyboardHapticFeedback

        // Read the pasteboard once — the content read triggers iOS's
        // "允许粘贴" prompt, so we gate on changeCount and share the
        // snapshot between the record and push paths. changeCount is
        // stamped only after the push completes (or in the no-server
        // early return) to avoid the record path blocking the push.
        let cc = UIPasteboard.general.changeCount
        let storedCC = store.loadLastSyncedChangeCount()
        let ccChanged = cc != storedCC
        let snap: DeviceClipboardSnapshot? = (ccChanged || force) ? PasteboardReader.snapshot() : nil
        log.info("sync: cc=\(cc) stored=\(storedCC ?? -1) ccChanged=\(ccChanged) force=\(force) snap=\(snap != nil) snapHash=\(snap?.clipboard.hash ?? "nil")")

        recordLocalClipboardIfNew(snap)
        if let snap, let payload = snap.payload, let hash = snap.clipboard.hash {
            store.saveImageData(hash: hash, data: payload)
        }
        reloadCards()

        // §5.3 from an extension: the keyboard never probes (its lifetime is
        // measured in keystrokes), so it layers the main app's last probe
        // verdict (`live_urls`, App Group) over pure shape order —
        // `preferredURLs` puts the probe-confirmed URL first and falls back
        // to `orderedURLs[0]` when no verdict exists for this profile.
        let server: ServerConfig? = {
            guard var cfg = servers.activeConfig else { return nil }
            cfg.urls = cfg.preferredURLs(
                live: store.loadLiveURL(configId: cfg.id),
                network: currentNetworkContext()
            )
            return cfg
        }()
        guard let server else {
            gate = .noServer
            store.saveLastSyncedChangeCount(cc)
            if force {
                lastError = String(localized: "尚未配置服务器，请先在主程序中添加")
                flashSync(.failure)
            }
            if gen == syncGeneration { isSyncing = false }
            return
        }
        gate = .ok
        serverLabel = server.displayLabel
        let trust = settings.trustInsecureCert
        ctx = (server, trust)

        isSyncing = true

        // ---- Uplink: push the device pasteboard if it carries new content.
        await pushDeviceClipboardIfNew(snap, changeCount: cc, server: server, trust: trust)
        guard gen == syncGeneration else { return }
        let didPush: Bool = { if case .pushed = pushStatus { return true } else { return false } }()
        log.info("sync uplink done: pushStatus=\(String(describing: self.pushStatus)) didPush=\(didPush)")
        reloadCards()

        // ---- Downlink: pull the server's latest *metadata* (small JSON) and
        // fold it into the history log if it's new. The payload (image /
        // overflow text) is fetched lazily on tap — never during this pass.
        do {
            let client = try SyncClipboardClient(server: server, trustInsecureCert: trust)
            let latest = try await client.getClipboard()
            guard gen == syncGeneration else { return }

            if didPush, let pushed = lastPushedEntry, Self.isSameContent(latest, pushed) {
                // The server's latest IS the entry we just pushed. The
                // server may compute a different profile hash for images
                // (it derives a different filename component), so we adopt
                // the server's hash as our watermark. This prevents both
                // this keyboard and the main app from re-pulling the same
                // content as a "new" entry.
                //
                // `isSameContent` gates the adoption: if another device
                // pushed between our PUT and this GET, blindly adopting the
                // returned hash would mark content we've NEVER seen as
                // "already synced" — swallowing it for the whole suite (the
                // main app would skip it: server hash == watermark) AND
                // letting the next push overwrite it on the server.
                if let serverHash = latest.hash, !serverHash.isEmpty {
                    log.info("sync post-push: adopting server hash \(serverHash.prefix(16))… (was \(self.store.loadLastSyncedHash()?.prefix(16) ?? "nil"))")
                    store.saveLastSyncedHash(serverHash)
                }
                lastError = nil
                flashSync(.success)
            } else {
                // Normal pull — including the "we pushed but another device
                // pushed right after" race, where `latest` is genuinely new
                // remote content that must surface, not be adopted.
                let historyHeadHash = store.loadHistory().first?.entry.hash
                log.info("sync pull: serverHash=\(latest.hash ?? "nil") serverType=\(latest.type.rawValue) historyHeadHash=\(historyHeadHash ?? "nil") lastSyncedHash=\(self.store.loadLastSyncedHash() ?? "nil")")
                let pulledNew = appendPulledIfNew(latest)
                log.info("sync pull result: pulledNew=\(pulledNew)")
                reloadCards()
                lastError = nil
                if force || didPush || pulledNew { flashSync(.success) }
            }
        } catch {
            guard gen == syncGeneration else { return }
            log.error("sync: failed — \(String(describing: error))")
            lastError = Self.message(for: error)
            flashSync(.failure)
        }

        if gen == syncGeneration { isSyncing = false }
    }

    /// Record the device pasteboard to the shared history log if it carries
    /// content we haven't seen. Does NOT stamp the changeCount watermark —
    /// that's deferred to pushDeviceClipboardIfNew so the push path isn't
    /// blocked by the record path having already stamped it.
    private func recordLocalClipboardIfNew(_ snap: DeviceClipboardSnapshot?) {
        guard let snap, let hash = snap.clipboard.hash?.uppercased() else { return }
        if hash == store.loadLastSyncedHash()?.uppercased() { return }
        if store.loadHistory().first?.entry.hash?.uppercased() == hash { return }
        store.appendHistory(entry: snap.clipboard, direction: .local)
    }

    /// Push the device pasteboard to the server if it carries new content.
    /// Stamps the changeCount watermark on all exit paths so the poll tick
    /// doesn't retry the same content.
    private func pushDeviceClipboardIfNew(
        _ snap: DeviceClipboardSnapshot?,
        changeCount cc: Int,
        server: ServerConfig,
        trust: Bool
    ) async {
        guard let snap, let hash = snap.clipboard.hash?.uppercased() else {
            log.info("push: snap nil or no hash → .none")
            store.saveLastSyncedChangeCount(cc)
            pushStatus = .none
            return
        }
        let lastHash = store.loadLastSyncedHash()?.uppercased()
        if hash == lastHash {
            log.info("push: hash==lastSyncedHash → .skipped (\(hash.prefix(16))…)")
            store.saveLastSyncedChangeCount(cc)
            pushStatus = .skipped
            return
        }
        log.info("push: uploading hash=\(hash.prefix(16))… lastSynced=\(lastHash?.prefix(16) ?? "nil") type=\(snap.clipboard.type.rawValue)")
        do {
            try await KeyboardUploader(store: store).upload(snap, to: server, trustInsecureCert: trust)
            store.saveLastSyncedChangeCount(cc)
            store.appendHistory(entry: snap.clipboard, direction: .pushed)
            pushStatus = .pushed(Self.summary(for: snap.clipboard))
            lastPushedEntry = snap.clipboard
            log.info("push: success")
        } catch {
            store.saveLastSyncedChangeCount(cc)
            pushStatus = .failed(Self.message(for: error))
            log.error("push: FAILED \(error)")
        }
    }

    /// Whether the server's `latest` is plausibly the entry we just pushed.
    /// Hash equality is conclusive; for images the server may rewrite the
    /// hash AND the filename (it derives its own name component), so fall
    /// back to type + size. Text compares the inline text itself.
    private static func isSameContent(_ server: Clipboard, _ pushed: Clipboard) -> Bool {
        guard server.type == pushed.type else { return false }
        if let sh = server.hash, let ph = pushed.hash,
           !sh.isEmpty, sh.uppercased() == ph.uppercased() {
            return true
        }
        switch server.type {
        case .text:          return server.text == pushed.text
        case .image, .file:  return server.size == pushed.size
        case .group:         return false
        }
    }

    /// Fold the server's freshly-pulled latest into the history log when it's
    /// genuinely new, so it surfaces as the head card. Skips kinds the
    /// keyboard can't act on (file/group) and empty text. `appendHistory`
    /// dedupes against the most-recent same-direction+hash entry; the extra
    /// "is it already the newest?" guard here also catches the just-pushed
    /// case (same hash, opposite direction) so a push isn't echoed as a pull.
    /// Returns `true` iff a genuinely new entry was appended.
    @discardableResult
    private func appendPulledIfNew(_ latest: Clipboard) -> Bool {
        guard let hash = latest.hash?.uppercased(), !hash.isEmpty else { return false }
        switch latest.type {
        case .text:
            if !latest.hasData && latest.text.isEmpty { return false }
        case .image:
            guard latest.hasData, latest.dataName != nil else { return false }
        case .file, .group:
            return false
        }
        if store.loadHistory().first?.entry.hash?.uppercased() == hash {
            return false
        }
        if hash == store.loadLastSyncedHash()?.uppercased() {
            return false
        }
        store.appendHistory(entry: latest, direction: .pulled)
        return true
    }

    /// Show a brief outcome badge on the refresh button, then clear it.
    /// Success lingers ~1.4s; failure a touch longer so it's noticed.
    private func flashSync(_ outcome: SyncFlash) {
        syncFlash = outcome
        flashTask?.cancel()
        flashTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(outcome == .success ? 1.4 : 2.0))
            if !Task.isCancelled { self?.syncFlash = nil }
        }
    }

    /// Rebuild the card row from the on-disk history log (newest-first,
    /// text + image only). Cheap enough to call after every sync half.
    private func reloadCards() {
        cards = store.loadHistory()
            .sorted { $0.timestamp > $1.timestamp }
            .compactMap(Self.card(from:))
    }

    private static func card(from item: ClipboardHistoryItem) -> Card? {
        let e = item.entry
        switch e.type {
        case .text:
            let isLink = looksLikeURL(e.text)
            return Card(
                id: item.id,
                kind: isLink ? .link : .text,
                entry: e,
                title: snippet(e.text),
                subtitle: isLink ? urlHost(e.text) : nil,
                time: Self.relativeShort(item.timestamp),
                sizeText: textCountText(e.size ?? e.text.count)
            )
        case .image:
            guard e.hasData, let name = e.dataName else { return nil }
            let rawExt = (name as NSString).pathExtension
            let ext = rawExt.isEmpty ? "png" : rawExt.lowercased()
            return Card(
                id: item.id,
                kind: .image,
                entry: e,
                title: String(localized: "图片"),
                subtitle: ext.uppercased(),
                time: Self.relativeShort(item.timestamp),
                sizeText: imageSizeText(byteCount: e.size ?? 0)
            )
        case .file, .group:
            return nil
        }
    }

    // MARK: - Card actions

    /// Act on a tapped card: insert text inline, or fetch + copy an image to
    /// the system pasteboard (a text field can't host an image inline).
    /// Long text / images fetch their payload here, on the tap, not during
    /// the auto-sync pass.
    ///
    /// Copying an image advances the pasteboard `changeCount`, and the
    /// follow-up `refresh()` pushes it to the server through the normal
    /// uplink — same "copy = sync" semantics as tapping a card in the main
    /// app. The watermark advances through the push path, so the shared
    /// `lastSyncedHash` invariant ("server latest == device == this hash")
    /// holds. The previous behavior wrote `saveLastSyncedHash` directly
    /// WITHOUT pushing, which left the shared watermark pointing at content
    /// the server never had as its latest — the main app's next tick would
    /// then mistake the server's unchanged latest for new remote content,
    /// re-pull it as a duplicate, and overwrite whatever the user had
    /// copied in the meantime.
    func activate(_ card: Card) {
        guard actingCardID == nil else { return }
        keyFeedback()
        let e = card.entry
        switch card.kind {
        case .text, .link:
            if e.hasData, let name = e.dataName {
                // §3.4 overflow: title shows only the preview; fetch the full
                // text file, then insert.
                fetchThen(card: card, name: name) { [weak self] data in
                    guard let self, let s = String(data: data, encoding: .utf8) else { return }
                    self.insertText(s)
                    self.flashActed(card.id)
                }
            } else {
                insertText(e.text)
                flashActed(card.id)
            }
        case .image:
            guard let name = e.dataName else { return }
            let rawExt = (name as NSString).pathExtension
            let ext = rawExt.isEmpty ? "png" : rawExt.lowercased()
            // Local-cache-first, mirroring `thumbnail(for:)`. If the card's
            // thumbnail rendered, the full original bytes are already in the
            // App Group cache (`ImageData/<hash>.dat`) — copy straight from
            // there, instantly and offline. Hitting `getFile(name:)` here was
            // both wasteful (the bytes are already local) and fragile: the
            // server's `file/<dataName>` is often gone by then, so the fetch
            // failed and — failure being swallowed — the copy silently did
            // nothing.
            if let hash = e.hash, let local = store.loadImageData(hash: hash), !local.isEmpty {
                copyImageToPasteboard(local, ext: ext, cardID: card.id)
            } else {
                // No local bytes (e.g. a server-pulled metadata entry whose
                // payload was never fetched) — fall back to the server, which
                // now surfaces fetch failures instead of swallowing them.
                fetchThen(card: card, name: name) { [weak self] data in
                    guard let self, !data.isEmpty else { return }
                    self.copyImageToPasteboard(data, ext: ext, cardID: card.id)
                }
            }
        }
    }

    /// Write image bytes to `UIPasteboard.general`, cache them under their
    /// content hash (so the app's offline preview finds them), surface the
    /// card at the history head, and push through the normal uplink. Shared by
    /// the local-cache-hit fast path and the server-fetch fallback in
    /// `activate`. Reading back our own just-written pasteboard never prompts.
    private func copyImageToPasteboard(_ data: Data, ext: String, cardID: UUID) {
        UIPasteboard.general.setData(data, forPasteboardType: PasteboardReader.uti(forExt: ext))
        store.saveImageData(hash: Clipboard.computeBytesHash(data), data: data)
        store.touchHistoryItem(id: cardID)
        flashActed(cardID)
        refresh()
    }

    /// Fetch a payload file by name from the last-synced server, then run
    /// `body` with its bytes on the main actor. Surfaces fetch failures via
    /// `lastError` (shown inline; the row stays put).
    private func fetchThen(card: Card, name: String, _ body: @escaping (Data) -> Void) {
        guard let ctx else { return }
        actingCardID = card.id
        Task { [weak self] in
            defer { self?.actingCardID = nil }
            do {
                let client = try SyncClipboardClient(server: ctx.server, trustInsecureCert: ctx.trust)
                let data = try await client.getFile(name: name)
                if Task.isCancelled { return }
                self?.lastError = nil
                body(data)
            } catch {
                if Task.isCancelled { return }
                log.error("fetchThen: getFile(name: \(name)) failed — \(error)")
                self?.lastError = Self.message(for: error)
                self?.flashSync(.failure)
            }
        }
    }

    private func flashActed(_ id: UUID) {
        actedCardID = id
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            if self?.actedCardID == id { self?.actedCardID = nil }
        }
    }

    // MARK: - Thumbnails

    /// Lazily fetch + downsample an image card's thumbnail. Cached by content
    /// hash; bounded by a per-image size guard so a huge original never blows
    /// the keyboard's memory budget (those fall back to a placeholder). The
    /// downsample decodes straight to ~`maxPixel` via ImageIO — the full
    /// bitmap is never realized.
    func thumbnail(for card: Card, maxPixel: CGFloat = 220) async -> UIImage? {
        guard card.kind == .image,
              let name = card.entry.dataName,
              let hash = card.entry.hash else { return nil }
        let key = hash as NSString
        if let cached = thumbnailCache.object(forKey: key) { return cached }
        if let size = card.entry.size, size > 8 * 1024 * 1024 { return nil }

        // Local cache first (App Group), then fall back to server.
        let data: Data
        if let local = store.loadImageData(hash: hash) {
            data = local
        } else {
            guard let ctx else { return nil }
            do {
                let client = try SyncClipboardClient(server: ctx.server, trustInsecureCert: ctx.trust)
                data = try await client.getFile(name: name)
                if Task.isCancelled { return nil }
                store.saveImageData(hash: hash, data: data)
            } catch {
                log.error("thumbnail: getFile failed — \(String(describing: error))")
                return nil
            }
        }
        guard let img = Self.downsample(data: data, maxPixel: maxPixel) else { return nil }
        thumbnailCache.setObject(img, forKey: key)
        return img
    }

    /// Decode `data` to a thumbnail no larger than `maxPixel` on its long
    /// edge, honoring EXIF orientation. ImageIO decodes directly to the
    /// requested size — the full-resolution bitmap is never allocated.
    private static func downsample(data: Data, maxPixel: CGFloat) -> UIImage? {
        let srcOpts = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let src = CGImageSourceCreateWithData(data as CFData, srcOpts) else { return nil }
        let thumbOpts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, thumbOpts as CFDictionary) else {
            return nil
        }
        return UIImage(cgImage: cg)
    }

    // MARK: - Link detection

    /// True for a trimmed, whitespace-free http(s) URL with a host. Kept
    /// strict so prose with a stray "www." doesn't masquerade as a link.
    private static func looksLikeURL(_ s: String) -> Bool {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, t.count <= 2048, !t.contains(where: \.isWhitespace) else { return false }
        guard let url = URL(string: t),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              url.host?.isEmpty == false else { return false }
        return true
    }

    private static func urlHost(_ s: String) -> String? {
        URL(string: s.trimmingCharacters(in: .whitespacesAndNewlines))?.host
    }

    // MARK: - Formatting helpers

    private static func snippet(_ text: String, limit: Int = 120) -> String {
        let collapsed = text
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if collapsed.count <= limit { return collapsed }
        return String(collapsed.prefix(limit)) + "…"
    }

    private static func summary(for clip: Clipboard) -> String {
        switch clip.type {
        case .text:  return snippet(clip.text, limit: 40)
        case .image: return String(localized: "图片")
        case .file:  return clip.dataName ?? String(localized: "文件")
        case .group: return String(localized: "内容")
        }
    }

    private static func textCountText(_ count: Int) -> String {
        String(localized: "\(count) 字")
    }

    private static func imageSizeText(byteCount: Int) -> String {
        guard byteCount > 0 else { return "" }
        let f = ByteCountFormatter()
        f.countStyle = .file
        return f.string(fromByteCount: Int64(byteCount))
    }

    /// "刚刚" inside ±5s, else the system relative formatter. Local to the
    /// extension — the app's `Date.relativeShort` lives in the main target.
    private static func relativeShort(_ date: Date) -> String {
        if abs(date.timeIntervalSinceNow) < 5 { return String(localized: "刚刚") }
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f.localizedString(for: date, relativeTo: Date())
    }

    private static func message(for error: Error) -> String {
        if let e = error as? SyncError { return message(for: e) }
        if let e = error as? LocalizedError, let d = e.errorDescription { return d }
        return String(localized: "同步失败")
    }

    /// User-facing copy for `SyncError`. Mirrors `SendClipboardIntent.errorMessage`
    /// (which lives in the app target and isn't visible to this extension).
    private static func message(for err: SyncError) -> String {
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

#if DEBUG
extension KeyboardModel {
    /// Seeds a populated card row for Xcode Previews — the keyboard can only
    /// be exercised on a real device, so previews are how the layout gets
    /// eyeballed. Thumbnails resolve to the placeholder (no `ctx`/network).
    static func previewReady() -> KeyboardModel {
        let m = KeyboardModel()
        m.hasFullAccess = true
        m.gate = .ok
        m.serverLabel = "家里的 NAS"
        m.syncFlash = .success
        m.cards = [
            Card(id: UUID(), kind: .text,
                 entry: Clipboard(type: .text, text: "明天上午 10 点开会,别忘了带上周的报表。", hasData: false, size: 18),
                 title: "明天上午 10 点开会,别忘了带上周的报表。", subtitle: nil, time: "刚刚", sizeText: "18 字"),
            Card(id: UUID(), kind: .link,
                 entry: Clipboard(type: .text, text: "https://uniclip.app/start", hasData: false, size: 25),
                 title: "https://uniclip.app/start", subtitle: "uniclip.app", time: "2 分钟前", sizeText: "25 字"),
            Card(id: UUID(), kind: .image,
                 entry: Clipboard(type: .image, text: "截屏", hasData: true, dataName: "shot.png", size: 1_240_000),
                 title: "图片", subtitle: "PNG", time: "5 分钟前", sizeText: "1.2 MB"),
            Card(id: UUID(), kind: .text,
                 entry: Clipboard(type: .text, text: "let name = \"Uni Clipboard\"", hasData: false, size: 27),
                 title: "let name = \"Uni Clipboard\"", subtitle: nil, time: "8 分钟前", sizeText: "27 字"),
        ]
        return m
    }

    static func previewEmpty() -> KeyboardModel {
        let m = KeyboardModel()
        m.hasFullAccess = true
        m.gate = .ok
        m.serverLabel = "家里的 NAS"
        return m
    }
}
#endif
