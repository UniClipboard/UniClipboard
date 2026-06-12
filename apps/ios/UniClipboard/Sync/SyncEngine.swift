import Foundation
import Observation
import OSLog
import SentryWithoutUIKit

private let log = Logger(subsystem: "app.uniclipboard", category: "sync")

/// Drives the auto-sync state machine. Cycle 9 product surface — replaces
/// the manual "推送" / "应用到本机" buttons with a 1Hz foreground tick that
/// converges both sides automatically.
///
/// State machine per `tick()`:
///
/// 1. `GET /SyncClipboard.json` (§2.1) for server metadata.
/// 2. If `server.hash != lastSyncedContentHash`, the server has new content:
///    - If we already fetched bytes for this hash and are waiting for the
///      user to flip the auto-apply toggle (`stagedServerHash == server.hash`),
///      no-op. Otherwise download, verify, and stage `vm.serverLatest`.
///    - If `appSettings.autoApplyServerChanges` is on, write to
///      `UIPasteboard.general` via `pasteboard.write`, advance
///      `lastSyncedContentHash` to the server hash, drop the staged hash.
///    - If off, set `state = .hasNewUnwritten`, keep the staged hash so
///      subsequent ticks don't re-download.
/// 3. Otherwise (server unchanged), check `vm.deviceClipboard.hash` against
///    `lastSyncedContentHash`. The observer is the source of truth — it
///    auto-reads on `UIPasteboard.changedNotification` and
///    `didBecomeActiveNotification`, so by the time the engine ticks, the
///    cached `current` reflects whatever the user just copied. If hashes
///    differ, the engine pushes via `vm.push()` (which itself does a
///    fresh `pasteboard.snapshot()` for bytes).
/// 4. On success, advance `lastSyncedContentHash` to the new content hash.
///
/// Conflict resolution is server-wins. When both sides changed inside the
/// same tick the server is processed first; on the next tick the device
/// pasteboard now matches the server (we just wrote it) and the hash dedup
/// short-circuits push, so we don't echo the server's content back to it.
///
/// Errors:
/// - `.authFailed` (401) → `state = .authFailed`, the loop pauses entirely.
///   Resume by calling `start()` again after the user fixes credentials.
/// - `.networkUnreachable` / `.timeout` / others → `state = .offlineRetrying`
///   and the next tick fires after `offlineBackoffSeconds` instead of the
///   normal cadence.
/// - `.notFound` (404) on server GET is the documented "empty server"
///   state and is treated as success — fall through to the push side.
@MainActor
@Observable
final class SyncEngine {
    enum State: Equatable {
        case idle
        case succeeded
        case hasNewUnwritten
        case offlineRetrying
        case authFailed
        /// Apply/Push of the same hash flipped too many times inside the
        /// `SyncLoopGuard` window — the loop is suspended until the user
        /// acknowledges via `acknowledgeLoopDetection()`. Set anywhere we
        /// advance `lastAppliedContentHash` / `lastPushedContentHash` and
        /// `loopGuard.tripped()` returns true.
        case loopDetected
    }

    private(set) var state: State = .idle
    private(set) var lastSyncedAt: Date?
    private(set) var lastError: SyncError?

    /// Server entry that was fetched but not written to UIPasteboard
    /// because `appSettings.autoApplyServerChanges == false`. UI can show
    /// it highlighted/expanded. `nil` when nothing is staged.
    private(set) var stagedEntry: Clipboard?

    /// True while a user-explicit tick (pull-to-refresh, toolbar refresh
    /// button) is in flight. Routine 1Hz ticks DO NOT flip this — the
    /// connector and toolbar reflect the last stable result state so users
    /// see "已同步" most of the time instead of a flickering "同步中…"
    /// every second.
    private(set) var isExplicitlyRefreshing: Bool = false

    @ObservationIgnored
    private weak var viewModel: AppViewModel?

    @ObservationIgnored
    private let store: SettingsStore

    @ObservationIgnored
    private var lastSyncedContentHash: String?

    /// The hash we most recently *wrote to UIPasteboard* via apply. Tracked
    /// separately from `lastSyncedContentHash` so the push gate has a
    /// second line of defense: even if some downstream layer (basename
    /// canonicalization, iOS pasteboard re-encoding) reports the device
    /// hash slightly differently from the server hash we just applied,
    /// `maybePush` won't push the freshly-applied content back. Cleared
    /// (along with `lastSyncedContentHash`) when the active server flips.
    @ObservationIgnored
    private var lastAppliedContentHash: String?

    /// Cycle detector. Records every apply / push and trips when the same
    /// hash flips direction too many times — see `SyncLoopGuard` for the
    /// state machine.
    @ObservationIgnored
    private var loopGuard = SyncLoopGuard()

    /// Hash we already downloaded but didn't write. Used to dedup the bytes
    /// fetch when auto-apply is off and the server hash hasn't changed.
    @ObservationIgnored
    private var stagedServerHash: String?

    @ObservationIgnored
    private var loopTask: Task<Void, Never>?

    @ObservationIgnored
    private var isTicking: Bool = false

    /// Client of the in-flight tick's server round-trip, held so
    /// `handleNetworkRouteChanged` / `handleEndpointChanged` can abort a
    /// request that's talking to a now-dead URL instead of letting it run
    /// out the request timeout — during which `isTicking` would stall
    /// explicit refreshes (the pull-to-refresh "stuck spinner" after a
    /// Wi-Fi → cellular flip). Cleared when the tick unwinds; the detached
    /// history-sync task may keep the client alive past that, which is
    /// fine — history sync is best-effort and swallows its own errors.
    @ObservationIgnored
    private var inFlightClient: SyncClipboardClient?

    /// Normal foreground cadence. Public for tests / debug overrides; not a
    /// user setting.
    @ObservationIgnored
    var normalCadenceSeconds: Double = 1.0

    /// Cadence applied while scene phase is `.inactive` — e.g. user
    /// pulled down Notification Center, took an incoming call,
    /// triggered the app switcher. The system pasteboard is still
    /// reachable in this phase, so we don't want to stop the loop
    /// entirely (a quick Control Center dismiss should resume sync
    /// without a perceptible pause). But running at full 1Hz during a
    /// minutes-long phone call wastes battery for no observable
    /// progress, so we throttle to 5s. `.active` transitions restore
    /// the 1Hz cadence on the next sleep.
    @ObservationIgnored
    var inactiveCadenceSeconds: Double = 5.0

    /// Whether the engine should run at the reduced `inactiveCadenceSeconds`
    /// cadence. Driven by the view layer's `scenePhase` observer.
    @ObservationIgnored
    var isSceneInactive: Bool = false

    /// Initial backoff applied to the FIRST network error. Subsequent
    /// consecutive errors double this up to `offlineBackoffMaxSeconds`
    /// (with ±20% jitter applied per-tick). A successful tick resets the
    /// backoff to this value. Public for tests that want to compress to
    /// zero.
    @ObservationIgnored
    var offlineBackoffSeconds: Double = 5.0

    /// Hard ceiling on the exponential backoff. 60s keeps a long
    /// outage from extending past one minute between retries — long
    /// enough to not hammer a dead server, short enough that a recovered
    /// network surfaces in the UI within roughly a minute of reconnect.
    @ObservationIgnored
    var offlineBackoffMaxSeconds: Double = 60.0

    /// Number of consecutive failed ticks. Drives the exponential
    /// backoff. Cleared on any successful tick (set via
    /// `markTickSucceeded`).
    @ObservationIgnored
    private var consecutiveFailures: Int = 0

    /// Minimum gap between successive §2.7 `POST /api/history/query`
    /// rounds. The live-clipboard tick runs every 1s, but the history
    /// API is a heavier (paginated) operation and changes rarely outside
    /// of explicit user activity — 30s captures cross-device edits
    /// quickly enough without hammering the server. Public for tests
    /// that want to compress it to zero.
    @ObservationIgnored
    var historySyncInterval: Double = 30.0

    /// Safety bound on the inner pagination loop. The empty-array
    /// sentinel is the documented end-of-list, so this only ever fires
    /// on a misbehaving server.
    @ObservationIgnored
    var historySyncMaxPages: Int = 50

    /// When the last successful (or attempted) `runHistorySyncIfDue` ran.
    /// nil triggers an immediate first sync on the next tick. Persisted
    /// to `SettingsStore` so an app cold-launch within the throttle
    /// window doesn't re-fire a full pagination — the previous behavior
    /// (in-memory only) re-ran the full §2.7 walk on every foreground,
    /// which is what landed dozens of POSTs in the server log per app
    /// open.
    @ObservationIgnored
    private var lastHistorySyncAt: Date?

    /// Mutex for the history-sync coroutine. The live-clipboard tick used
    /// to `await` the history pagination inline, which blocked the 1Hz
    /// loop for as long as the server's history took to paginate
    /// (observed: ~5s for a 20-page cold start). We now spawn it as a
    /// detached task, but two consecutive ticks could otherwise overlap;
    /// this flag drops the second entrant. Cleared in `defer`.
    @ObservationIgnored
    private var isHistorySyncing: Bool = false

    init(viewModel: AppViewModel, store: SettingsStore) {
        self.viewModel = viewModel
        self.store = store
        self.lastSyncedContentHash = store.loadLastSyncedHash()
        self.lastHistorySyncAt = store.loadLastHistorySyncAt()
    }

    /// Begin the polling loop. Idempotent — calling while already running
    /// is a no-op. Call from `scenePhase == .active`. Resets `.authFailed`
    /// on the assumption that the caller is retrying after a credential
    /// fix. Does NOT reset `.loopDetected` — that one requires explicit
    /// user acknowledgment via `acknowledgeLoopDetection()`, otherwise the
    /// next scene-phase flip would silently re-enter the loop.
    func start() {
        guard loopTask == nil else { return }
        if state == .authFailed { state = .idle }
        if state == .loopDetected { return }
        loopTask = Task { @MainActor [weak self] in
            while let self, !Task.isCancelled {
                await self.tick()
                let interval = self.cadenceSeconds()
                if interval.isInfinite { break }
                let nanos = UInt64(max(0.05, interval) * 1_000_000_000)
                try? await Task.sleep(nanoseconds: nanos)
            }
        }
    }

    /// Cancel the polling loop. Call from `scenePhase == .background` and
    /// before tearing down the engine.
    func stop() {
        loopTask?.cancel()
        loopTask = nil
    }

    /// Run a tick now, ignoring the normal cadence and any offline backoff.
    /// Marked as an explicit refresh — UI shows a spinner until it
    /// completes. Used by the toolbar refresh button.
    func forceTickNow() {
        log.info("forceTickNow: state=\(String(describing: self.state), privacy: .public) isTicking=\(self.isTicking, privacy: .public)")
        Task { @MainActor in await self.tick(explicit: true) }
    }

    /// Awaitable explicit tick — use from `.refreshable` so the SwiftUI
    /// pull-to-refresh control keeps its native spinner up until the
    /// tick actually finishes (rather than flashing for a frame).
    func explicitRefresh() async {
        log.info("explicitRefresh: state=\(String(describing: self.state), privacy: .public) isTicking=\(self.isTicking, privacy: .public)")
        await tick(explicit: true)
    }

    /// Called by the UI when the user manually applies a staged server
    /// entry (e.g. taps "应用" on the home-screen banner). Advances
    /// `lastSyncedContentHash` to the staged hash and clears the staged
    /// state so the next tick exits `.hasNewUnwritten` instead of
    /// re-observing the same server hash and re-staging it forever.
    /// No-op when nothing is staged.
    func markStagedApplied() {
        guard let hash = stagedServerHash else { return }
        advanceSynced(to: hash)
        lastAppliedContentHash = hash.uppercased()
        stagedServerHash = nil
        stagedEntry = nil
        state = .succeeded
        lastSyncedAt = .now
        lastError = nil
    }

    /// User-side dismissal of the loop-detected banner. Wipes the cycle
    /// detector's buffer so the next legitimate sync doesn't re-trip
    /// instantly, drops back to `.idle`, and restarts the loop. Safe to
    /// call when the state isn't `.loopDetected` — clears the buffer and
    /// restarts as a defensive no-op.
    func acknowledgeLoopDetection() {
        loopGuard.reset()
        state = .idle
        lastError = nil
        start()
    }

    /// Clear runtime state without touching persisted hash. Useful when the
    /// user switches active server — the new server has its own content
    /// timeline.
    func resetRuntimeState() {
        stagedServerHash = nil
        stagedEntry = nil
        lastAppliedContentHash = nil
        loopGuard.reset()
        lastError = nil
        state = .idle
        // The old server's backoff window must not throttle the new one.
        nextNetworkAttemptAt = nil
        consecutiveFailures = 0
        // Force the next tick to hit /api/history/query immediately —
        // the new server's `lastModified` watermark is unrelated to the
        // old one's, so we have to refetch from scratch (the caller
        // clears `vm.historyWatermark` separately). Clear on disk too:
        // if the app crashes/backgrounds between server switch and the
        // first post-switch tick, init would otherwise reload the old
        // server's timestamp and silently throttle the new server's
        // first pull.
        lastHistorySyncAt = nil
        store.saveLastHistorySyncAt(nil)
    }

    /// Called by `AppViewModel` when the *effective* active server changes —
    /// whether the user picked a different one (§5.2) or a Wi-Fi auto-switch
    /// rule took effect on a network change (§5.3 `effectiveActiveConfig`).
    /// `AppViewModel` owns the "did the effective id actually change?"
    /// comparison (it's the one holding the current SSID); by the time we're
    /// called the switch is real. The new server has its own content +
    /// history timeline, so drop all per-server runtime state, restart a
    /// possibly-paused loop, and force an immediate tick so the switch
    /// surfaces without waiting out the 1 Hz cadence.
    ///
    /// Note `resetRuntimeState` also clears `.loopDetected`, so the `start()`
    /// is unconditional (the loop was stopped when that breaker tripped).
    /// The separate ".authFailed → restart on same-server credential edit"
    /// case stays in `AppViewModel.servers.didSet` — it isn't a server
    /// change, so it doesn't belong here.
    func handleActiveServerChanged() {
        resetRuntimeState()
        lastSyncedContentHash = nil
        store.saveLastSyncedHash(nil)
        // §2.7 watermark is per-server too — clear it so the next tick pulls
        // the new server's full history page by page.
        viewModel?.historyWatermark = nil
        start()
        forceTickNow()
    }

    /// The network path changed (Wi-Fi ↔ cellular, VPN up/down) — called by
    /// `AppViewModel.handleNetworkChanged` the moment NWPathMonitor fires.
    /// Anything in flight was built against a URL chosen for the OLD
    /// network and may now be a blackhole: cancel it so the tick unwinds in
    /// milliseconds instead of running out the request timeout. The backoff
    /// accumulated against the old path doesn't transfer either — failures
    /// of a dead route say nothing about the new one. No forced tick here:
    /// with the gate cleared the 1Hz cadence retries immediately against
    /// shape order (the VM nils `liveURL` before calling us), and the §5.3
    /// probe fires `handleEndpointChanged` if its verdict flips the URL.
    func handleNetworkRouteChanged() {
        inFlightClient?.cancelInFlight()
        consecutiveFailures = 0
        nextNetworkAttemptAt = nil
    }

    /// The §5.3 probe confirmed a reachable live URL different from the
    /// previous one — a recovery signal, act NOW rather than waiting out
    /// cadence or leftover backoff: cancel whatever is still talking to
    /// the old URL, clear the gate, and tick immediately (the per-tick
    /// client rebuild picks the new `urls[0]` up automatically). Same
    /// profile, same content timeline — sync watermarks deliberately
    /// untouched, unlike `handleActiveServerChanged`.
    func handleEndpointChanged() {
        inFlightClient?.cancelInFlight()
        consecutiveFailures = 0
        nextNetworkAttemptAt = nil
        forceTickNow()
    }

    private func cadenceSeconds() -> Double {
        switch state {
        case .authFailed:       return .infinity
        case .loopDetected:     return .infinity
        // `.offlineRetrying` deliberately keeps the NORMAL cadence: the
        // tick still observes the local pasteboard every second (so a
        // fresh copy lands a history card immediately even while the
        // server is unreachable). The network half is gated separately by
        // `nextNetworkAttemptAt`, which is what actually backs off.
        default:                return isSceneInactive ? inactiveCadenceSeconds : normalCadenceSeconds
        }
    }

    /// Earliest moment the next NETWORK attempt may run. Set on a failed
    /// tick from `currentBackoffSeconds()`; cleared on success. Ticks that
    /// land inside the window still run the local pasteboard observation
    /// but skip the server round-trip — backoff throttles the network, not
    /// the user's own clipboard. Explicit refreshes bypass the window.
    @ObservationIgnored
    private var nextNetworkAttemptAt: Date?

    /// Exponential backoff with ±20% jitter. `consecutiveFailures == 1`
    /// → base; doubles up to `offlineBackoffMaxSeconds`. Jitter prevents
    /// thundering-herd retry against a server that recovered at a known
    /// moment. Reduced cadence is good for the server AND the device
    /// battery; the UI's "重试中" indicator already conveys the wait.
    private func currentBackoffSeconds() -> Double {
        let failures = max(1, consecutiveFailures)
        // 2^(failures-1) capped at 2^6 = 64× the base — anything past
        // ~6 doublings overshoots the max anyway; the cap protects
        // against Double overflow if failures grew pathologically.
        let exponent = min(failures - 1, 6)
        let multiplier = pow(2.0, Double(exponent))
        let base = offlineBackoffSeconds * multiplier
        let capped = min(base, offlineBackoffMaxSeconds)
        let jitter = Double.random(in: 0.8 ... 1.2)
        return capped * jitter
    }

    private func tick(explicit: Bool = false) async {
        if explicit {
            // An explicit refresh must NOT be dropped by the `isTicking`
            // guard — the toolbar/pull-to-refresh spinner depends on
            // `isExplicitlyRefreshing` going true synchronously, and if a
            // routine tick happens to be in flight when the user taps,
            // we'd otherwise leave them staring at a static UI for up to
            // the network timeout.
            //
            // Show the spinner immediately, then wait for any in-flight
            // routine tick to finish before starting ours. Polling sleep
            // (50ms) is fine: routine ticks are short by design (<1s
            // typically), and we yield the actor between iterations so
            // the routine tick can actually progress.
            isExplicitlyRefreshing = true
            while isTicking {
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        } else {
            guard !isTicking else { return }
        }
        isTicking = true
        defer {
            isTicking = false
            inFlightClient = nil
            if explicit { isExplicitlyRefreshing = false }
        }
        guard let vm = viewModel else {
            state = .idle
            return
        }
        // Pasteboard observation runs BEFORE the server guard so local
        // clipboard changes are recorded even without a configured server.
        if vm.appSettings.autoPushDeviceChanges {
            vm.pollPasteboardIfChanged()
            // Auto-push ON: record any new device clipboard content locally,
            // regardless of whether a server is configured. Skip content we
            // wrote to the pasteboard ourselves (apply / reapply) — it's
            // already in history under its original provenance.
            if let device = vm.deviceClipboard,
               let hash = device.hash?.uppercased(),
               hash != lastAppliedContentHash,
               !isHashInRecentHistory(vm: vm, hash: hash) {
                // Seed BEFORE appending: the card renders (and its
                // thumbnail loader fires) the moment the append lands,
                // and locally-produced bytes must already be on disk by
                // then — a local screenshot shows instantly, independent
                // of whether/when the upload happens.
                vm.seedLocalPayloadCache(for: device)
                vm.appendHistory(entry: device, direction: .local)
            }
        } else {
            vm.pollPasteboardDetection()
        }
        guard let server = vm.activeServer else {
            state = .idle
            return
        }
        if state == .authFailed || state == .loopDetected { return }
        // Network backoff gate. Local pasteboard observation above already
        // ran — a copy made while the server is unreachable still lands its
        // history card on the next 1s tick. Explicit refreshes punch through.
        if !explicit, let next = nextNetworkAttemptAt, Date() < next { return }
        log.debug("tick: explicit=\(explicit, privacy: .public) state=\(String(describing: self.state), privacy: .public) url=\(server.url, privacy: .public) consecutiveFailures=\(self.consecutiveFailures, privacy: .public)")
        // Cross-process re-sync: the Share Extension writes
        // `lastSyncedContentHash` directly into the App Group when it
        // finishes a push (see CLAUDE.md "Share Extension write
        // coordination"). Without re-reading here, the engine's in-memory
        // copy from `init` is stale and the next tick would re-apply the
        // just-shared entry back onto the device — the exact ping-pong
        // that the coordination key was meant to prevent.
        let persisted = store.loadLastSyncedHash()
        if !hashesEqual(persisted, lastSyncedContentHash) {
            lastSyncedContentHash = persisted?.uppercased()
        }
        // Note: routine 1Hz ticks don't transition to a "syncing"
        // intermediate state — the visible `state` stays at its last
        // stable value, so the connector strip doesn't flicker every
        // second between "同步中…" and "已同步". Explicit refreshes
        // surface their progress via `isExplicitlyRefreshing` instead.
        do {
            let client = try SyncClipboardClient(
                server: server,
                trustInsecureCert: vm.appSettings.trustInsecureCert
            )
            inFlightClient = client
            // Pull side first (server-wins).
            let serverEntryOrNil: Clipboard?
            do {
                serverEntryOrNil = try await client.getClipboard()
            } catch let e as SyncError where e.kind == .notFound {
                // Empty server — leave serverLatest as-is, fall through.
                serverEntryOrNil = nil
            }
            if let serverEntry = serverEntryOrNil,
               let serverHash = serverEntry.hash, !serverHash.isEmpty,
               let deviceHash = vm.deviceClipboard?.hash, !deviceHash.isEmpty,
               hashesEqual(serverHash, deviceHash) {
                // Truth gate: server latest and device clipboard hold
                // identical content — already converged, no matter what the
                // watermark says. Extensions write the shared watermark too
                // (keyboard / share / intents), and a desynced watermark
                // must not make us re-pull content the device already has
                // (clobbering a fresh local copy) or re-push content the
                // server already has. Repair the watermark and move on.
                advanceSynced(to: serverHash)
                lastAppliedContentHash = serverHash.uppercased()
                stagedServerHash = nil
                stagedEntry = nil
                state = .succeeded
                lastSyncedAt = .now
                lastError = nil
            } else if let serverEntry = serverEntryOrNil,
               !hashesEqual(serverEntry.hash, lastSyncedContentHash) {
                try await processServerNew(serverEntry, client: client, vm: vm)
            } else {
                // Push side.
                try await maybePush(client: client, vm: vm, server: server)
            }
            // Best-effort incremental history sync (§2.7), throttled to
            // `historySyncInterval`. Internal failures don't escalate to
            // engine state — history is a strict superset of the live
            // clipboard, and the live tick above is the user-visible
            // sync signal. Spawned as a detached task so the 1Hz live
            // loop keeps running while history paginates: prior shape
            // `await runHistorySyncIfDue(...)` froze live polling for
            // the entire duration of the §2.7 walk (5+ seconds on cold
            // start with 20-page histories).
            let engine = self
            Task { @MainActor in
                await engine.runHistorySyncIfDue(client: client, vm: vm)
            }
            // Any path that reached here (apply-success, push-success,
            // device==nil pass-through, hash-equal pass-through) is a
            // healthy tick — drop the backoff counter so a recovered
            // network reverts to 1Hz cadence on the next sleep.
            if consecutiveFailures > 0 {
                log.info("tick: recovered after \(self.consecutiveFailures, privacy: .public) failures, state=\(String(describing: self.state), privacy: .public)")
                SentrySDK.logger.info("sync recovered", attributes: ["failedTicks": consecutiveFailures])
            }
            consecutiveFailures = 0
            nextNetworkAttemptAt = nil
        } catch let e as SyncError where e.kind == .authFailed {
            log.error("tick: auth failed, stopping loop")
            SentrySDK.logger.warn("sync auth failed (401), polling paused")
            state = .authFailed
            lastError = e
            stop()
        } catch let e as SyncError where e.kind == .cancelled {
            // Deliberate abort: a network-path change or live-URL flip
            // invalidated the URL this tick was talking to, and the VM
            // cancelled the request mid-flight. Not a failure of server or
            // network — no backoff, no state flip, no re-probe kick; the
            // follow-up tick (1Hz cadence or `handleEndpointChanged`'s
            // forced one) runs against the fresh `urls[0]`.
            log.info("tick: aborted — request cancelled by network-route/endpoint change")
        } catch let e as SyncError {
            consecutiveFailures += 1
            nextNetworkAttemptAt = Date().addingTimeInterval(currentBackoffSeconds())
            log.error("tick: SyncError kind=\(String(describing: e.kind), privacy: .public) consecutiveFailures=\(self.consecutiveFailures, privacy: .public) underlying=\(e.underlying ?? "nil", privacy: .public)")
            // Mirror only the offline TRANSITION to Sentry — each backoff
            // retry repeats the same failure and would just be log spam.
            if consecutiveFailures == 1 {
                SentrySDK.logger.warn(
                    "sync went offline",
                    attributes: ["errorKind": String(describing: e.kind)]
                )
            }
            state = .offlineRetrying
            lastError = e
            // A network-shaped failure may mean the *URL* died, not the
            // server (left the LAN, Tailscale dropped) — ask the VM to
            // re-probe the candidates so the next tick's client builds
            // against a path that works (§5.3 Layer 2). Non-forced: the
            // VM's debounce absorbs the 1Hz failure cadence. A successful
            // tick is the implicit confirmation of the current URL, so
            // there's no mirror call on the happy path.
            switch e.kind {
            case .networkUnreachable, .connectTimeout, .receiveTimeout:
                vm.kickLiveEndpointRefresh()
            default:
                break
            }
        } catch {
            consecutiveFailures += 1
            nextNetworkAttemptAt = Date().addingTimeInterval(currentBackoffSeconds())
            log.error("tick: unexpected error consecutiveFailures=\(self.consecutiveFailures, privacy: .public): \(String(describing: error), privacy: .public)")
            // Non-SyncError out of the tick is bug-grade — every expected
            // failure mode is mapped to SyncError upstream.
            SentrySDK.capture(error: error) { scope in
                scope.setTag(value: "sync-tick", key: "path")
            }
            state = .offlineRetrying
            lastError = SyncError(kind: .networkUnreachable, underlying: "\(error)")
            vm.kickLiveEndpointRefresh()
        }
    }

    private func processServerNew(
        _ entry: Clipboard,
        client: SyncClipboardClient,
        vm: AppViewModel
    ) async throws {
        // Dedup: if we already fetched this hash and the user just hasn't
        // flipped auto-apply yet, skip the bytes round-trip.
        //
        // Hashless server entries (spec violation — §4 requires SHA-256)
        // dedup by full-Clipboard equality against `stagedEntry`. Without
        // this, `lastSyncedContentHash` never advances (advanceSynced
        // guards on non-nil hash), so the main router always re-enters
        // here and we'd re-append + re-prefetch every tick.
        let entryHasHash = !(entry.hash ?? "").isEmpty
        let alreadyStaged: Bool = entryHasHash
            ? (stagedServerHash.map { hashesEqual($0, entry.hash) } ?? false)
            : (stagedEntry == entry)
        if !alreadyStaged {
            vm.serverLatest = entry
            // First time we see this server entry — log it on the Home
            // list as a pulled event. The staged branch above is a
            // re-observation of the same hash, so we'd otherwise double
            // log every tick the user leaves auto-apply off.
            vm.appendHistory(entry: entry, direction: .pulled)
            // Prefetch the payload bytes into PayloadCache so a later
            // tap-to-preview opens without a network round-trip. The
            // helper short-circuits on cellular (unless the user opted
            // in) and on entries without a stable cache key. The
            // device-apply path below races safely — PayloadCache's
            // per-profileId `pending` table dedups in-flight fetches.
            vm.prefetchAttachmentIfEligible(entry)
        }
        if vm.appSettings.autoApplyServerChanges && entryHasHash {
            // Auto-apply requires a hash. Hashless entries always fall
            // into the stage-only branch below so the user can decide
            // (the manual apply path likely fails verification anyway,
            // but that's a UI / server-bug surfacing concern, not a
            // reason for the engine to loop forever).
            //
            // Reuse the throwing apply variant so this path doesn't
            // depend on the sticky `vm.applyError` field — that field
            // can outlive a single tick when the UI's "应用" button
            // failed earlier, and reading it after the call would
            // mis-attribute that prior error to the engine.
            do {
                _ = try await vm.applyServerToDeviceThrowing()
            } catch let err as SyncError {
                // Park the entry in the staged slot so the next tick goes
                // through the `alreadyStaged` short-circuit — without this,
                // every subsequent tick re-runs `vm.serverLatest = entry`
                // + `prefetchAttachmentIfEligible` for the same (failed)
                // entry. `appendHistory` already dedups via the last-hash
                // check, but the prefetch/serverLatest churn is wasted
                // work. State stays at .offlineRetrying via the outer
                // catch so the user sees the failure.
                stagedServerHash = entry.hash
                stagedEntry = entry
                throw err
            }
            advanceSynced(to: entry.hash)
            lastAppliedContentHash = entry.hash?.uppercased()
            stagedServerHash = nil
            stagedEntry = nil
            state = .succeeded
            lastSyncedAt = .now
            lastError = nil
            loopGuard.record(.pulled, hash: entry.hash)
            if loopGuard.tripped() { tripLoopBreaker() }
        } else if !alreadyStaged {
            // Stage but don't write. Update stagedServerHash to dedup next
            // tick. lastSyncedContentHash stays where it was — until the
            // user toggles auto-apply on or copies something new locally,
            // we're "stuck" on hasNewUnwritten by design.
            stagedServerHash = entry.hash
            stagedEntry = entry
            state = .hasNewUnwritten
            lastSyncedAt = .now
            lastError = nil
        }
        // alreadyStaged && (auto-apply off || hashless) → no-op tick.
        // Don't bump `lastSyncedAt` — the UI's "上次同步 N 秒前" should
        // reflect actual progress, not every routine re-observation of
        // an already-staged entry.
    }

    private func maybePush(
        client: SyncClipboardClient,
        vm: AppViewModel,
        server: ServerConfig
    ) async throws {
        guard vm.appSettings.autoPushDeviceChanges else {
            // Consent-push mode (default): the engine never reads the
            // pasteboard or pushes on its own — device→server happens only
            // when the user taps the Home `PasteButton` (see `consentPush`).
            // Nothing to do here; the tick is healthy.
            state = .succeeded
            lastError = nil
            return
        }
        guard let device = vm.deviceClipboard else {
            // Observer hasn't surfaced anything yet (cold start, env-hook
            // returned nil, etc). Nothing to push — and nothing to claim
            // as a "sync" either, so leave `lastSyncedAt` where it was.
            // Writing `.now` here surfaces a misleading "刚刚同步" while
            // the device side has literally never been observed.
            state = .succeeded
            lastError = nil
            return
        }
        if hashesEqual(device.hash, lastSyncedContentHash) {
            // Already synced.
            state = .succeeded
            lastSyncedAt = .now
            lastError = nil
            return
        }
        if hashesEqual(device.hash, lastAppliedContentHash) {
            // Defense (B): what's on the pasteboard is content WE wrote —
            // an applied server entry, or a re-applied history item whose
            // pasteboard form hashes differently from the entry it came
            // from (§3.4 overflow preview, file/group filename-as-text).
            // Treat it as already synced and DON'T push it back; doing so
            // would overwrite the server's entry with the device-side
            // rendering and start the apply↔push pong.
            state = .succeeded
            lastSyncedAt = .now
            lastError = nil
            return
        }
        // `pushReturningEntry()` returns the pushed entry on success,
        // nil on documented silent skips (no snapshot / unpushable type
        // / in-flight isPushing), and throws on real failure — so the
        // engine no longer has to look at `vm.pushError`, which is
        // sticky and may carry an error from a prior UI-initiated push.
        let pushed: Clipboard?
        do {
            pushed = try await vm.pushReturningEntry()
        } catch let err as SyncError {
            throw err
        }
        if let pushed {
            advanceSynced(to: pushed.hash)
            vm.appendHistory(entry: pushed, direction: .pushed)
            loopGuard.record(.pushed, hash: pushed.hash)
            if loopGuard.tripped() { tripLoopBreaker() }
            // Mirror the Share Extension's donation so iOS Sharing
            // Suggestions ranks this server higher even when the user
            // never explicitly invokes the share sheet — auto-sync
            // counts as "sent to this server" too. Fire-and-forget so
            // the next tick isn't blocked on IPC.
            Task { await ShareIntentDonation.donateSend(to: server, clipboard: pushed) }
        }
        // No serverLatest after push = silent skip (no active config / no
        // pushable snapshot / type unsupported). Don't advance hash, but
        // succeeded state lets the loop continue.
        state = .succeeded
        lastSyncedAt = .now
        lastError = nil
    }

    /// Push content the user explicitly handed us via the Home
    /// `PasteButton`. Distinct from `maybePush`: the bytes are already in
    /// hand (the system paste control granted access without a prompt), so
    /// we PUT directly via `vm.pushSnapshot` — no `UIPasteboard` read. On
    /// success we run the same bookkeeping `maybePush` does (advance synced
    /// hash so the next pull doesn't echo it, log history, cycle-guard,
    /// Siri donation) plus `adoptConsentPush` so the device card reflects it
    /// and the push hint clears. Errors land in `state`/`lastError` exactly
    /// like a routine push failure, so the existing Home issue chrome
    /// surfaces them. Runs regardless of `autoPushDeviceChanges` — it's the
    /// default push path, not gated by the auto-push opt-in.
    func consentPush(_ snapshot: DeviceClipboardSnapshot) async {
        guard let vm = viewModel else { return }

        let entry = snapshot.clipboard
        // Always record locally first, regardless of server availability —
        // seeding the byte caches before the append so the card's
        // thumbnail finds them the moment it renders.
        vm.seedLocalPayloadCache(from: snapshot)
        vm.appendHistory(entry: entry, direction: .local)
        vm.adoptConsentPush(entry)

        guard let server = vm.activeServer else { return }
        if state == .authFailed || state == .loopDetected { return }
        do {
            guard let pushed = try await vm.pushSnapshot(snapshot) else { return }
            advanceSynced(to: pushed.hash)
            lastAppliedContentHash = pushed.hash?.uppercased()
            vm.updateHistoryDirection(hash: pushed.hash, to: .pushed)
            loopGuard.record(.pushed, hash: pushed.hash)
            if loopGuard.tripped() { tripLoopBreaker(); return }
            state = .succeeded
            lastSyncedAt = .now
            lastError = nil
            consecutiveFailures = 0
            Task { await ShareIntentDonation.donateSend(to: server, clipboard: pushed) }
        } catch let e as SyncError where e.kind == .authFailed {
            state = .authFailed
            lastError = e
            stop()
        } catch let e as SyncError {
            consecutiveFailures += 1
            state = .offlineRetrying
            lastError = e
        } catch {
            consecutiveFailures += 1
            state = .offlineRetrying
            lastError = SyncError(kind: .networkUnreachable, underlying: "\(error)")
        }
    }

    /// Called when the UI re-applies a history entry onto the device
    /// pasteboard, BEFORE the corresponding server push completes.
    /// Records the hash of what was actually written so the tick neither
    /// re-pushes the self-written content nor logs it as a new `.local`
    /// copy. Deliberately does NOT advance `lastSyncedContentHash` — the
    /// server still holds the previous content until the PUT lands, and
    /// advancing early would make the next tick mistake the server's
    /// unchanged latest for new remote content (re-pulling and reverting
    /// the user's selection).
    func noteReapplyWritten(deviceHash: String?) {
        guard let deviceHash, !deviceHash.isEmpty else { return }
        lastAppliedContentHash = deviceHash.uppercased()
    }

    /// Called after a successful server push of a re-applied history entry.
    /// Advances the synced hash and clears any staged state so the next
    /// tick doesn't pull the same entry back.
    func advanceSyncedForReapply(to hash: String?) {
        advanceSynced(to: hash)
        stagedServerHash = nil
        stagedEntry = nil
        state = .succeeded
        lastSyncedAt = .now
        lastError = nil
        consecutiveFailures = 0
    }

    private func advanceSynced(to hash: String?) {
        // A nil/empty hash means the entry is unverifiable (hashMatches treats
        // it as a pass per §4.4). We can still mark it "synced for the moment"
        // but can't dedup future ticks — leave lastSyncedContentHash alone so
        // the next tick re-evaluates. Rare in practice (well-formed servers
        // always emit hash).
        guard let hash, !hash.isEmpty else { return }
        let normalized = hash.uppercased()
        lastSyncedContentHash = normalized
        store.saveLastSyncedHash(normalized)
    }

    /// Pull the §2.7 history pages incrementally and merge into
    /// `vm.history`. Best-effort: any failure is swallowed and the next
    /// due window will retry. Throttled by `historySyncInterval`.
    ///
    /// Re-entrancy: spawned from `tick()` as a detached Task, so two
    /// adjacent ticks could overlap if a paginated walk takes longer
    /// than `normalCadenceSeconds`. `isHistorySyncing` drops the
    /// second entrant — we'd otherwise issue interleaved page-1/page-2
    /// requests across two concurrent walks for the same server.
    ///
    /// Cold-start strategy: when no watermark exists yet (first-ever
    /// install, freshly-switched server) we deliberately fetch ONLY the
    /// first page and seed the watermark from it. The next due tick
    /// then pulls strictly-newer records via `modifiedAfter`, so we
    /// never re-paginate the full server-side history. The user loses
    /// nothing in the live clipboard path (which is the headline
    /// product surface), and the §2.7 history list back-fills
    /// page-by-page as new records arrive. Without this branch, a
    /// server with N pages of accumulated history hammered the API
    /// with N POSTs on every fresh install / server switch.
    private func runHistorySyncIfDue(client: SyncClipboardClient, vm: AppViewModel) async {
        if let last = lastHistorySyncAt,
           Date().timeIntervalSince(last) < historySyncInterval {
            return
        }
        guard !isHistorySyncing else { return }
        isHistorySyncing = true
        // Always advance the throttle, even on failure — otherwise a
        // server that 500s on /api/history/query would have every 1Hz
        // tick spam-retry the endpoint. Persist via SettingsStore so a
        // cold launch within the throttle window doesn't restart the
        // walk from scratch.
        defer {
            isHistorySyncing = false
            lastHistorySyncAt = .now
            store.saveLastHistorySyncAt(lastHistorySyncAt)
        }

        let watermark = vm.historyWatermark
        let isColdStart = (watermark == nil)
        var maxModified: Date = watermark ?? .distantPast
        var page = 1
        while page <= historySyncMaxPages {
            let records: [HistoryRecord]
            do {
                records = try await client.queryHistory(
                    HistoryQuery(page: page, modifiedAfter: watermark)
                )
            } catch {
                // Best-effort. Swallow — the engine's `state` and
                // `lastError` remain tied to the live-clipboard path.
                return
            }
            if records.isEmpty {
                // Cold-start + empty server: seed the watermark to
                // `.now` so the next due window doesn't re-enter the
                // cold-start branch and probe page 1 again every 30s
                // forever. A subsequent server-side write will surface
                // through the normal `modifiedAfter` delta path.
                if isColdStart {
                    vm.historyWatermark = .now
                }
                break
            }
            for record in records {
                vm.mergeHistoryRecord(record)
                if let lm = record.lastModified, lm > maxModified {
                    maxModified = lm
                }
            }
            if isColdStart {
                // Seed the watermark from page 1 and let future ticks
                // back-fill via `modifiedAfter`. Without this break, an
                // accumulated history (typical for any server that's
                // been running for a while) would issue N back-to-back
                // POSTs on every fresh install / server switch.
                break
            }
            page += 1
        }
        if maxModified > (watermark ?? .distantPast) {
            vm.historyWatermark = maxModified
        }
    }

    /// Park the engine in `.loopDetected` and stop the polling loop.
    /// Idempotent — re-entering the trip path while already tripped is a
    /// no-op. Recovery is `acknowledgeLoopDetection()` from the UI.
    private func tripLoopBreaker() {
        guard state != .loopDetected else { return }
        log.error("loop breaker tripped — suspending sync until user acknowledges")
        // The loop guard tripping means our apply/push dedup failed —
        // that's a logic bug worth an issue, not just a log line.
        SentrySDK.capture(message: "sync loop breaker tripped") { scope in
            scope.setLevel(.warning)
            scope.setTag(value: "sync-loop-guard", key: "path")
        }
        state = .loopDetected
        lastError = SyncError(
            kind: .networkUnreachable,
            underlying: "auto-sync loop detected — same content alternated apply/push too many times"
        )
        stop()
    }

    private func isHashInRecentHistory(vm: AppViewModel, hash: String) -> Bool {
        guard let first = vm.history.first else { return false }
        return first.entry.hash?.uppercased() == hash
    }

    private func hashesEqual(_ a: String?, _ b: String?) -> Bool {
        switch (a, b) {
        case (nil, nil): return true
        case (let l?, let r?): return l.uppercased() == r.uppercased()
        default: return false
        }
    }
}
