import Foundation
import OSLog

private let log = Logger(subsystem: "app.uniclipboard", category: "store")

/// Persists `ServerConfigList` and `AppSettings` under the keys defined in
/// `AppSettings.PersistenceKey` (spec: `docs/SYNC_PROTOCOL.md` §5.4, §5.5).
///
/// Pure Foundation — lives in the SwiftPM `Models` target so it can be
/// unit-tested via `swift test`.
///
/// Corruption policy: if a stored JSON blob fails to decode, the store
/// returns the type's default ( empty list / `AppSettings.defaults`). This
/// matches the forward-compat philosophy of `AppSettings.init(from:)` —
/// stored data must never block app startup.
public final class SettingsStore: @unchecked Sendable {
    /// App Group container shared between the main app and the Share
    /// Extension. Keep in sync with the `application-groups` entitlement
    /// on both targets.
    public static let appGroupID = "group.app.uniclipboard.UniClipboard"

    /// Filename of the file-backed `last_synced_content_hash` under
    /// `containerURL`. Plain text, UTF-8, contains a single uppercase
    /// 64-char hex SHA-256 (or is absent for `nil`). The file lives outside
    /// `UserDefaults` because `cfprefsd` caches the App Group suite
    /// per-process and lags cross-process writes — the Share Extension's
    /// `saveLastSyncedHash` would otherwise not be visible to the main
    /// app's `SyncEngine` for an indeterminate window after the
    /// extension's PUT, letting the engine pull the just-pushed entry back
    /// to the device (the §5.4 cross-process ping-pong this key exists
    /// to prevent). `Data.write(to:options:.atomic)` is `tmp + rename`,
    /// so concurrent readers see either the old value or the new one,
    /// never a half-written file.
    static let lastSyncedHashFilename = "last_synced_hash"

    /// Filename of the file-backed `last_known_ssid` under `containerURL`.
    /// Plain UTF-8, holds one normalized SSID (§5.1) or is absent for "no
    /// Wi-Fi / unknown". File-backed (not `UserDefaults`) for the same
    /// cross-process-freshness reason as `lastSyncedHashFilename`: the main
    /// app writes it on every SSID change and the keyboard extension reads
    /// it to resolve the on-demand active server, and `cfprefsd` caches the
    /// App Group suite per-process.
    static let lastKnownSSIDFilename = "last_known_ssid"

    /// Filename of the file-backed live-URL map under `containerURL`. JSON
    /// `{configId: url}` — for each profile, the candidate URL the main
    /// app's most recent §5.3 probe confirmed reachable. Only the main app
    /// writes it (extensions never probe); the keyboard extension reads it
    /// as its best guess before falling back to pure shape order. File-
    /// backed (not `UserDefaults`) for the same cross-process-freshness
    /// reason as the two files above.
    static let liveURLsFilename = "live_urls"

    private let defaults: UserDefaults
    private let containerURL: URL
    private let encoder: JSONEncoder
    private let decoder: JSONDecoder

    /// - Parameters:
    ///   - defaults: when nil (the default), the store opens the App Group
    ///     suite (`appGroupID`) and one-shot-migrates any existing keys
    ///     from `.standard` on first use. Falls back to `.standard` if the
    ///     App Group entitlement isn't active. Tests pass an explicit
    ///     ephemeral `UserDefaults(suiteName:)`.
    ///   - containerURL: directory holding file-backed state (currently
    ///     just `last_synced_hash`). When nil, resolves to the App Group
    ///     container URL. Tests inject a unique tmp dir so file state is
    ///     isolated per case.
    public init(defaults: UserDefaults? = nil, containerURL: URL? = nil) {
        let chosenDefaults: UserDefaults
        if let defaults {
            chosenDefaults = defaults
        } else if let suite = UserDefaults(suiteName: SettingsStore.appGroupID) {
            SettingsStore.migrateFromStandardIfNeeded(into: suite)
            chosenDefaults = suite
        } else {
            chosenDefaults = .standard
        }
        self.defaults = chosenDefaults

        let chosenContainer: URL
        if let containerURL {
            chosenContainer = containerURL
        } else if let groupURL = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: SettingsStore.appGroupID
        ) {
            chosenContainer = groupURL
        } else {
            // No App Group entitlement (SwiftPM test harness, ad-hoc CLI).
            // A unique tmp dir keeps file state isolated and disposable —
            // any consumer that lacks the entitlement is by definition not
            // sharing state with another process, so process-uniqueness
            // matches what they get from `.standard` UserDefaults above.
            chosenContainer = FileManager.default.temporaryDirectory
                .appendingPathComponent("UniClipboardStore-\(UUID().uuidString)", isDirectory: true)
        }
        try? FileManager.default.createDirectory(
            at: chosenContainer,
            withIntermediateDirectories: true
        )
        self.containerURL = chosenContainer
        self.encoder = JSONEncoder()
        self.decoder = JSONDecoder()

        // One-shot UserDefaults → file migration for `last_synced_hash`.
        // Idempotent: only fires when the file is absent AND the legacy key
        // is present, so a re-launch after the migration is a no-op.
        migrateLastSyncedHashToFileIfNeeded()
    }

    /// One-shot migration from `.standard` to the App Group suite. Runs
    /// the first time we open the suite after the App Group entitlement
    /// is added: copies known keys over and removes them from `.standard`.
    /// Idempotent — if any known key already exists in the suite the
    /// migration is considered done and skipped, so a re-install can't be
    /// overridden by a stale `.standard` blob.
    private static func migrateFromStandardIfNeeded(into suite: UserDefaults) {
        let keys = [
            AppSettings.PersistenceKey.serverConfigList,
            AppSettings.PersistenceKey.appSettings,
            AppSettings.PersistenceKey.lastSyncedContentHash,
            AppSettings.PersistenceKey.legacyServerConfig,
        ]
        for key in keys where suite.object(forKey: key) != nil {
            return
        }
        let standard = UserDefaults.standard
        var migrated = 0
        for key in keys {
            guard let value = standard.object(forKey: key) else { continue }
            suite.set(value, forKey: key)
            standard.removeObject(forKey: key)
            migrated += 1
        }
        if migrated > 0 {
            log.info("migrateFromStandardIfNeeded: moved \(migrated, privacy: .public) keys from .standard to the App Group suite")
        }
    }

    /// One-shot lift of `last_synced_content_hash` from `UserDefaults` to
    /// the file backend. Runs at every `init` but is idempotent: the file's
    /// presence alone short-circuits the copy, and the legacy key is
    /// cleared only after the file write succeeds so a crash mid-migration
    /// retries on next launch instead of losing the hash.
    private func migrateLastSyncedHashToFileIfNeeded() {
        let url = lastSyncedHashFileURL
        if (try? url.checkResourceIsReachable()) == true { return }
        guard let legacy = defaults.string(forKey: AppSettings.PersistenceKey.lastSyncedContentHash) else {
            return
        }
        let normalized = legacy.uppercased()
        guard writeLastSyncedHashFile(normalized) else { return }
        defaults.removeObject(forKey: AppSettings.PersistenceKey.lastSyncedContentHash)
    }

    private var lastSyncedHashFileURL: URL {
        containerURL.appendingPathComponent(SettingsStore.lastSyncedHashFilename, isDirectory: false)
    }

    @discardableResult
    private func writeLastSyncedHashFile(_ hash: String) -> Bool {
        let data = Data(hash.utf8)
        do {
            try data.write(to: lastSyncedHashFileURL, options: [.atomic])
            return true
        } catch {
            return false
        }
    }

    // MARK: - ServerConfigList

    /// Load the server list, performing one-shot legacy migration (§5.5)
    /// if `server_config_list` is absent and `server_config` is present.
    public func loadServers() -> ServerConfigList {
        if let data = defaults.data(forKey: AppSettings.PersistenceKey.serverConfigList) {
            if let list = try? decoder.decode(ServerConfigList.self, from: data) {
                return list
            }
            // Corruption policy returns the empty default — but losing the
            // whole server list is the worst silent failure this store can
            // produce, so leave a loud trace.
            log.fault("loadServers: server_config_list blob (\(data.count, privacy: .public) bytes) failed to decode — returning empty list")
            return ServerConfigList()
        }

        if let legacyData = defaults.data(forKey: AppSettings.PersistenceKey.legacyServerConfig),
           let legacy = try? decoder.decode(LegacyServerConfig.self, from: legacyData) {
            let migrated = legacy.migrated()
            saveServers(migrated)
            defaults.removeObject(forKey: AppSettings.PersistenceKey.legacyServerConfig)
            log.info("loadServers: migrated legacy server_config to server_config_list (§5.5)")
            return migrated
        }

        return ServerConfigList()
    }

    public func saveServers(_ list: ServerConfigList) {
        guard let data = try? encoder.encode(list) else { return }
        defaults.set(data, forKey: AppSettings.PersistenceKey.serverConfigList)
    }

    // MARK: - AppSettings

    public func loadAppSettings() -> AppSettings {
        guard let data = defaults.data(forKey: AppSettings.PersistenceKey.appSettings) else {
            return .defaults
        }
        if let settings = try? decoder.decode(AppSettings.self, from: data) {
            return settings
        }
        log.fault("loadAppSettings: app_settings blob (\(data.count, privacy: .public) bytes) failed to decode — using defaults")
        return .defaults
    }

    public func saveAppSettings(_ settings: AppSettings) {
        guard let data = try? encoder.encode(settings) else { return }
        defaults.set(data, forKey: AppSettings.PersistenceKey.appSettings)
    }

    // MARK: - Last-synced content hash (cycle 9)

    /// Load the most-recent content hash the sync engine confirmed both
    /// sides shared. `nil` on first launch or after the engine resets.
    ///
    /// Backed by a plain-text file under the App Group container, not
    /// `UserDefaults`. See `lastSyncedHashFilename` for why.
    public func loadLastSyncedHash() -> String? {
        guard let data = try? Data(contentsOf: lastSyncedHashFileURL),
              let raw = String(data: data, encoding: .utf8) else {
            return nil
        }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed.uppercased()
    }

    /// Persist the latest synced-content hash. Pass `nil` to clear it (e.g.
    /// when the user switches active server).
    ///
    /// Atomic write — readers in other processes see either the prior value
    /// or the new one, never a partial file. This is the cross-process
    /// half of the §5.4 anti-ping-pong guard; the temporal half (writing
    /// the hash *before* publishing the entry to the server) lives in the
    /// caller (e.g. `ShareUploader.upload`).
    public func saveLastSyncedHash(_ hash: String?) {
        if let hash, !hash.isEmpty {
            writeLastSyncedHashFile(hash.uppercased())
        } else {
            try? FileManager.default.removeItem(at: lastSyncedHashFileURL)
        }
    }

    // MARK: - Last-known Wi-Fi SSID (auto-switch overlay, cross-process)

    private var lastKnownSSIDFileURL: URL {
        containerURL.appendingPathComponent(SettingsStore.lastKnownSSIDFilename, isDirectory: false)
    }

    /// The most recent normalized Wi-Fi SSID the main app observed. The app
    /// writes it whenever the SSID changes (`AppViewModel.handleSSIDChanged`);
    /// the keyboard extension reads it to pick the on-demand active server
    /// (`ServerConfigList.effectiveActiveConfig`) without itself holding the
    /// wifi-info entitlement or Location authorization. `nil` ⇒ no Wi-Fi /
    /// unknown / never written. Re-normalized on read so a value that no
    /// longer passes §5.1 (shouldn't happen — we normalize on write) is
    /// treated as absent.
    public func loadLastKnownSSID() -> String? {
        guard let data = try? Data(contentsOf: lastKnownSSIDFileURL),
              let raw = String(data: data, encoding: .utf8) else {
            return nil
        }
        return ServerConfig.normalizeSSID(raw)
    }

    /// Persist the current Wi-Fi SSID. Normalized per §5.1; pass `nil`/empty
    /// (or anything that normalizes to `nil`, e.g. an Android privacy
    /// placeholder) to clear it — which a reader then sees as "no network",
    /// collapsing `effectiveActiveConfig` to the manual baseline. Atomic
    /// write so a cross-process reader never sees a half-written name.
    public func saveLastKnownSSID(_ ssid: String?) {
        if let normalized = ServerConfig.normalizeSSID(ssid) {
            try? Data(normalized.utf8).write(to: lastKnownSSIDFileURL, options: [.atomic])
        } else {
            try? FileManager.default.removeItem(at: lastKnownSSIDFileURL)
        }
    }

    // MARK: - Live URL per profile (§5.3 probe result, cross-process)

    private var liveURLsFileURL: URL {
        containerURL.appendingPathComponent(SettingsStore.liveURLsFilename, isDirectory: false)
    }

    /// The probe-confirmed URL for `configId`, or nil when the last probe
    /// found nothing reachable (or never ran). Readers must validate the
    /// value against the config's current `urls` — an edit may have removed
    /// the candidate since the probe wrote it.
    public func loadLiveURL(configId: String) -> String? {
        loadLiveURLMap()[configId]
    }

    /// Persist (or with nil, clear) the probe-confirmed URL for `configId`.
    /// Main-app only — extensions read, never write. Atomic write, same
    /// guarantee as the sibling files: a cross-process reader sees either
    /// the old map or the new one.
    public func saveLiveURL(configId: String, _ url: String?) {
        var map = loadLiveURLMap()
        if map[configId] == url { return }
        map[configId] = url
        if map.isEmpty {
            try? FileManager.default.removeItem(at: liveURLsFileURL)
            return
        }
        guard let data = try? encoder.encode(map) else { return }
        try? data.write(to: liveURLsFileURL, options: [.atomic])
    }

    private func loadLiveURLMap() -> [String: String] {
        guard let data = try? Data(contentsOf: liveURLsFileURL) else { return [:] }
        // Corruption policy as everywhere in this store: an undecodable
        // blob reads as "no live URL known", never blocks the caller.
        return (try? decoder.decode([String: String].self, from: data)) ?? [:]
    }

    // MARK: - Clipboard history (cycle 11)

    /// Load the persisted clipboard observation log. Returns `[]` on cold
    /// launch or when the stored JSON fails to decode (forward-compat with
    /// the rest of the store's corruption policy — never block startup on
    /// a bad blob).
    public func loadHistory() -> [ClipboardHistoryItem] {
        guard let data = defaults.data(forKey: AppSettings.PersistenceKey.clipboardHistory) else {
            return []
        }
        return (try? decoder.decode([ClipboardHistoryItem].self, from: data)) ?? []
    }

    /// Persist the clipboard observation log. Callers cap the size before
    /// calling — this method writes whatever it's handed. An empty array
    /// is still encoded (rather than removing the key) so a subsequent
    /// load round-trips to `[]` and the corruption fallback never fires.
    public func saveHistory(_ items: [ClipboardHistoryItem]) {
        guard let data = try? encoder.encode(items) else { return }
        defaults.set(data, forKey: AppSettings.PersistenceKey.clipboardHistory)
    }

    /// Append one observation to the shared history log (App Group),
    /// newest-first, deduped against the most-recent same-direction+hash
    /// entry, and capped. Mirrors `AppViewModel.appendHistory` so an
    /// extension (keyboard / share) that pushes or applies content while the
    /// main app is suspended still lands a row the user will see — the app
    /// reconciles the on-disk log on its next foreground.
    ///
    /// Process-safety is load-modify-save (last writer wins). Only one
    /// extension runs at a time and the host app is suspended while a
    /// keyboard runs in another app, so concurrent writers are not a
    /// practical concern on iPhone; the app's foreground merge covers the
    /// iPad-multitasking edge.
    public func appendHistory(
        entry: Clipboard,
        direction: ClipboardHistoryItem.Direction,
        at timestamp: Date = Date(),
        cap: Int = 200
    ) {
        var items = loadHistory()
        // Same content already at the head → never insert a duplicate row,
        // regardless of direction. Upgrade `.local` provenance to
        // pushed/pulled in place; keep the stronger direction otherwise.
        if let hash = entry.hash,
           let last = items.first,
           last.entry.hash == hash {
            if direction != .local, last.direction != direction {
                items[0].direction = direction
                saveHistory(items)
            }
            return
        }
        items.insert(
            ClipboardHistoryItem(entry: entry, timestamp: timestamp, direction: direction),
            at: 0
        )
        if items.count > cap { items = Array(items.prefix(cap)) }
        saveHistory(items)
    }

    /// Move the history item with `id` to the head of the log by stamping
    /// its timestamp to now. Used by keyboard tap-to-copy so the row
    /// surfaces first (matching the main app's reapply semantics) and the
    /// uplink's "already at head?" dedup recognizes the copied entry.
    public func touchHistoryItem(id: UUID) {
        var items = loadHistory()
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        var item = items.remove(at: idx)
        item.timestamp = Date()
        items.insert(item, at: 0)
        saveHistory(items)
    }

    // MARK: - Pasteboard change-count watermark (keyboard uplink)

    /// The `UIPasteboard.changeCount` the keyboard last synced. Lets the
    /// keyboard's uplink skip the *content* read (which fires iOS's
    /// "允许粘贴" prompt) when nothing has been copied since — reading
    /// `changeCount` itself is free and never prompts. `nil` on cold start.
    public func loadLastSyncedChangeCount() -> Int? {
        defaults.object(forKey: AppSettings.PersistenceKey.lastSyncedChangeCount) as? Int
    }

    public func saveLastSyncedChangeCount(_ value: Int) {
        defaults.set(value, forKey: AppSettings.PersistenceKey.lastSyncedChangeCount)
    }

    /// Load the incremental-sync watermark (the largest `lastModified`
    /// seen on any prior §2.7 page). Returns `nil` on cold launch or if
    /// the stored string fails to parse — both are treated as "fetch
    /// everything next round".
    public func loadHistoryWatermark() -> Date? {
        guard let s = defaults.string(forKey: AppSettings.PersistenceKey.historyModifiedAfter),
              !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return nil }
        if let d = Self.fractionalISOFormatter.date(from: s) { return d }
        if let d = Self.plainISOFormatter.date(from: s) { return d }
        return nil
    }

    /// Persist the watermark. Pass `nil` to clear (e.g. when switching
    /// servers — the new server's `lastModified` timeline is unrelated
    /// to the old one).
    public func saveHistoryWatermark(_ date: Date?) {
        if let date {
            defaults.set(Self.fractionalISOFormatter.string(from: date),
                         forKey: AppSettings.PersistenceKey.historyModifiedAfter)
        } else {
            defaults.removeObject(forKey: AppSettings.PersistenceKey.historyModifiedAfter)
        }
    }

    /// Load the "we last ran the §2.7 throttle window at" timestamp.
    /// `nil` means "we've never run it (or just switched servers)" —
    /// the engine treats nil as immediately-due. Unlike the watermark,
    /// this is purely a rate-limit hint; out-of-date values just cause
    /// one extra (incremental) pull, not data loss.
    public func loadLastHistorySyncAt() -> Date? {
        guard let s = defaults.string(forKey: AppSettings.PersistenceKey.lastHistorySyncAt),
              !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return nil }
        if let d = Self.fractionalISOFormatter.date(from: s) { return d }
        if let d = Self.plainISOFormatter.date(from: s) { return d }
        return nil
    }

    /// Persist the §2.7 throttle timestamp. Pass `nil` to clear (server
    /// switch — the new server's history should be pulled immediately
    /// even if the old server's throttle window hasn't elapsed).
    public func saveLastHistorySyncAt(_ date: Date?) {
        if let date {
            defaults.set(Self.fractionalISOFormatter.string(from: date),
                         forKey: AppSettings.PersistenceKey.lastHistorySyncAt)
        } else {
            defaults.removeObject(forKey: AppSettings.PersistenceKey.lastHistorySyncAt)
        }
    }

    // MARK: - Image data cache (App Group, shared with keyboard)

    private var imageCacheDir: URL {
        containerURL.appendingPathComponent("ImageData", isDirectory: true)
    }

    public func loadImageData(hash: String) -> Data? {
        let file = imageCacheDir.appendingPathComponent("\(hash.uppercased()).dat")
        return try? Data(contentsOf: file)
    }

    public func saveImageData(hash: String, data: Data) {
        try? FileManager.default.createDirectory(at: imageCacheDir, withIntermediateDirectories: true)
        let file = imageCacheDir.appendingPathComponent("\(hash.uppercased()).dat")
        try? data.write(to: file, options: .atomic)
    }

    // MARK: - ISO formatters (shared between watermark + future date keys)

    private static let fractionalISOFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let plainISOFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()
}
