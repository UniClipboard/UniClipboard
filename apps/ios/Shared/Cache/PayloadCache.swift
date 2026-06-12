import Foundation
#if canImport(UniClipboardModels)
// SwiftPM build: `SettingsStore.appGroupID` lives in a sibling target.
// Xcode app-target builds compile everything as one module so the
// `canImport` check is false there.
import UniClipboardModels
#endif

/// Content-addressed byte cache for clipboard payloads (image bytes, long-text
/// overflow bodies). Lives in the App Group container so both the main app and
/// the Share Extension can read/write the same files.
///
/// Files are named by §2.8 `profileId` (`"<Type>-<HASH>"`) — same form
/// `HistoryRecord.profileId` produces and `SyncClipboardClient.getHistoryPayload`
/// consumes. Lookup is therefore stable across the device-local round-trip
/// without any extra mapping table.
///
/// Eviction is LRU by file mtime: every read touches the file's modification
/// date, and `write` runs an inline sweep that deletes the oldest files until
/// total occupied bytes ≤ `maxBytes`. Cap is injected for tests; production
/// callers pass the App Group container URL + a hard cap (currently 200 MiB).
///
/// Concurrency: an `actor` rather than a `@MainActor` class so the Share
/// Extension can use it without bouncing through the main thread. A small
/// internal `Semaphore` bounds concurrent in-flight fetches to 3 — enough
/// to overlap a handful of pulls without saturating the link, low enough
/// that a burst of history-sync entries can't queue dozens of TCP streams.
/// Concurrent `fetchAndStore` callers for the same `profileId` dedup: the
/// first caller's `Task` is stored in `pending` and subsequent callers
/// `await` its value instead of starting a second download.
public actor PayloadCache {
    private let directory: URL
    /// Disk cap. Mutable so the Settings UI can shrink/grow it at
    /// runtime via `setMaxBytes(_:)`; the setter runs an immediate
    /// eviction sweep so a shrink frees disk on the spot.
    private var maxBytes: Int
    private let semaphore: Semaphore
    private var pending: [String: Task<Data, Error>] = [:]

    public init(directory: URL, maxBytes: Int, concurrencyLimit: Int = 3) {
        self.directory = directory
        self.maxBytes = maxBytes
        self.semaphore = Semaphore(limit: concurrencyLimit)
        try? FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
    }

    /// Update the cap and immediately evict if the new cap is below
    /// current occupancy. No-op when the cap is unchanged.
    public func setMaxBytes(_ newValue: Int) {
        guard newValue != maxBytes else { return }
        maxBytes = newValue
        evictIfOverCapacity()
    }

    /// Read-only accessor for the current cap. Useful for tests and
    /// for the Settings UI's "max bytes" readout.
    public func currentMaxBytes() -> Int { maxBytes }

    /// Return cached bytes if present, `nil` otherwise. A hit also bumps the
    /// file's `contentModificationDate` to "now" so the LRU sweep treats this
    /// entry as recently used.
    public func read(profileId: String) -> Data? {
        guard isValidKey(profileId) else { return nil }
        let url = directory.appendingPathComponent(profileId)
        guard let bytes = try? Data(contentsOf: url) else { return nil }
        touchMtime(url)
        return bytes
    }

    /// Write bytes atomically, mark the file as backup-excluded, then run an
    /// LRU sweep if we're over `maxBytes`.
    @discardableResult
    public func write(profileId: String, bytes: Data) throws -> URL {
        guard isValidKey(profileId) else {
            throw CacheError.invalidKey(profileId)
        }
        var url = directory.appendingPathComponent(profileId)
        try bytes.write(to: url, options: .atomic)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
        evictIfOverCapacity()
        return url
    }

    /// Remove one entry. No-op if missing.
    public func delete(profileId: String) {
        guard isValidKey(profileId) else { return }
        let url = directory.appendingPathComponent(profileId)
        try? FileManager.default.removeItem(at: url)
    }

    /// Remove every file in the cache directory. Used by the "清除缓存"
    /// settings action.
    public func purgeAll() {
        guard let entries = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: nil
        ) else { return }
        for entry in entries {
            try? FileManager.default.removeItem(at: entry)
        }
    }

    /// Sum of file sizes in the directory.
    public func totalSize() -> Int {
        listEntries().reduce(0) { $0 + $1.size }
    }

    /// Read-or-fetch with dedup. If the file is on disk, return it. Otherwise,
    /// if another caller is already fetching the same `profileId`, await their
    /// result. Otherwise, take a semaphore slot, run `fetcher`, write the
    /// result, and return.
    ///
    /// The inner unstructured `Task` does NOT inherit the caller's cancellation
    /// — that's intentional. Two prefetch callers racing the same id should
    /// share work; cancelling one shouldn't tear down the other's view.
    public func fetchAndStore(
        profileId: String,
        fetcher: @Sendable @escaping () async throws -> Data
    ) async throws -> Data {
        if let cached = read(profileId: profileId) {
            return cached
        }
        if let inflight = pending[profileId] {
            return try await inflight.value
        }
        let task = Task<Data, Error> { [weak self, semaphore] in
            await semaphore.acquire()
            defer { Task { await semaphore.release() } }
            let bytes = try await fetcher()
            if let self {
                try await self.completeFetch(profileId: profileId, bytes: bytes)
            }
            return bytes
        }
        pending[profileId] = task
        do {
            let bytes = try await task.value
            return bytes
        } catch {
            pending.removeValue(forKey: profileId)
            throw error
        }
    }

    // MARK: internal helpers

    /// Called by a `fetchAndStore` Task body once `fetcher` returns. Re-enters
    /// the actor so the write + pending cleanup land on the serial queue.
    private func completeFetch(profileId: String, bytes: Data) throws {
        defer { pending.removeValue(forKey: profileId) }
        try write(profileId: profileId, bytes: bytes)
    }

    private func evictIfOverCapacity() {
        var entries = listEntries()
        var total = entries.reduce(0) { $0 + $1.size }
        guard total > maxBytes else { return }
        entries.sort { $0.mtime < $1.mtime }
        for entry in entries {
            if total <= maxBytes { break }
            try? FileManager.default.removeItem(at: entry.url)
            total -= entry.size
        }
    }

    private func listEntries() -> [Entry] {
        let keys: [URLResourceKey] = [.contentModificationDateKey, .fileSizeKey, .isRegularFileKey]
        guard let urls = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: keys
        ) else { return [] }
        return urls.compactMap { url in
            guard let values = try? url.resourceValues(forKeys: Set(keys)),
                  values.isRegularFile == true,
                  let size = values.fileSize,
                  let mtime = values.contentModificationDate
            else { return nil }
            return Entry(url: url, size: size, mtime: mtime)
        }
    }

    private func touchMtime(_ url: URL) {
        // `URL.setResourceValues(contentModificationDate:)` silently no-ops on
        // some macOS hosts (observed in `swift test` runs). `FileManager`'s
        // POSIX-flavored setattr is the dependable path.
        try? FileManager.default.setAttributes(
            [.modificationDate: Date()],
            ofItemAtPath: url.path
        )
    }

    private func isValidKey(_ key: String) -> Bool {
        !key.isEmpty
            && !key.contains("/")
            && !key.contains("\\")
            && key != "."
            && key != ".."
    }

    private struct Entry {
        let url: URL
        let size: Int
        let mtime: Date
    }

    public enum CacheError: Error, Equatable {
        case invalidKey(String)
    }
}

/// Bounded concurrency primitive. Async-friendly counting semaphore: `acquire`
/// suspends when the slot count is exhausted and resumes when a previous
/// holder calls `release`. Implemented as an actor so the inflight counter
/// and waiter queue are accessed serially without locks.
public extension PayloadCache {
    /// Process-wide shared cache. Points at the App Group container's
    /// `payloads/` subdirectory so the main app and the Share Extension
    /// see the same files. Falls back to a tempdir-backed cache when the
    /// App Group entitlement isn't active (SwiftPM `swift test` host).
    ///
    /// 200 MiB hard cap. The settings UI for tuning this lands in a
    /// later PR; until then the cap is hardcoded.
    static let shared: PayloadCache = {
        let container = FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: SettingsStore.appGroupID)
            ?? FileManager.default.temporaryDirectory
                .appendingPathComponent("uniclipboard-payloads-fallback", isDirectory: true)
        return PayloadCache(
            directory: container.appendingPathComponent("payloads", isDirectory: true),
            maxBytes: 200 * 1024 * 1024
        )
    }()
}

private actor Semaphore {
    private let limit: Int
    private var inflight = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(limit: Int) {
        precondition(limit > 0)
        self.limit = limit
    }

    func acquire() async {
        if inflight < limit {
            inflight += 1
            return
        }
        await withCheckedContinuation { cont in
            waiters.append(cont)
        }
    }

    func release() {
        if waiters.isEmpty {
            inflight -= 1
        } else {
            // Hand the slot directly to the next waiter — no inflight churn.
            let next = waiters.removeFirst()
            next.resume()
        }
    }
}
