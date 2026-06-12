import Foundation

/// Uploads a single device-pasteboard `DeviceClipboardSnapshot` to the active
/// SyncClipboard server. The keyboard's analog of `ShareUploader` — same §3.5
/// file-first PUT sequence (payload bytes first, metadata second) and the same
/// pre-PUT `lastSyncedContentHash` watermark.
///
/// Writing the watermark *before* `putClipboard` is deliberate (see the
/// matching note in `ShareUploader`): `putClipboard` is the instant the new
/// hash becomes visible to every other client — the main app's `SyncEngine`
/// GETs `/SyncClipboard.json` at 1Hz. If we wrote the hash *after* the PUT, a
/// concurrent tick could see `server.hash != lastSyncedContentHash` and pull
/// the entry we just pushed back onto the device pasteboard (the "one bounce"
/// loop, which would also re-fire the "允许粘贴" prompt for no benefit).
struct KeyboardUploader {
    let store: SettingsStore

    init(store: SettingsStore = SettingsStore()) {
        self.store = store
    }

    func upload(
        _ snapshot: DeviceClipboardSnapshot,
        to server: ServerConfig,
        trustInsecureCert: Bool
    ) async throws {
        let client = try SyncClipboardClient(server: server, trustInsecureCert: trustInsecureCert)
        let entry = snapshot.clipboard

        if entry.hasData, let payload = snapshot.payload, let name = entry.dataName {
            try await client.putFile(name: name, body: payload)
        }
        try await client.putClipboard(entry)
        // Write the hash watermark AFTER a confirmed PUT — not before.
        // Writing before opens a race: pollTick can cancel this task
        // mid-PUT, leaving a stale watermark that causes the next cycle
        // to skip the push entirely.
        if let hash = entry.hash, !hash.isEmpty {
            store.saveLastSyncedHash(hash)
        }
    }
}
