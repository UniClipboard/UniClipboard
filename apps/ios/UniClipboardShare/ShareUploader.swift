import Foundation
import OSLog

private let log = Logger(subsystem: "app.uniclipboard", category: "share")

/// Uploads a single `ShareItem` to the active SyncClipboard server.
/// Lives in the Share Extension target — owns the §3.5 file-first PUT
/// sequence (file bytes first, metadata second) so the main app's sync
/// engine, when it next ticks, sees a fully-consistent server state.
///
/// Writes `lastSyncedContentHash` to the App-Group `SettingsStore` after
/// a successful push. This is what keeps the main app's `SyncEngine` from
/// interpreting the just-pushed entry as "server has new content" and
/// echoing it back to the device pasteboard on next tick (which would
/// trigger iOS's "Allow Paste" prompt — see CLAUDE.md notes on engine
/// dedup against `lastSyncedContentHash`).
struct ShareUploader {
    let store: SettingsStore

    init(store: SettingsStore = SettingsStore()) {
        self.store = store
    }

    func upload(_ item: ShareItem, to server: ServerConfig, trustInsecureCert: Bool) async throws {
        let client = try SyncClipboardClient(server: server, trustInsecureCert: trustInsecureCert)
        let (entry, payload) = build(from: item)
        log.info("upload: start \(item.kindLabel, privacy: .public) bytes=\(item.byteCount, privacy: .public) hasData=\(entry.hasData, privacy: .public)")

        if entry.hasData, let payload, let name = entry.dataName {
            try await client.putFile(name: name, body: payload)
            log.debug("upload: §3.5 file PUT done")
            if let hash = entry.hash, !hash.isEmpty {
                let profileId = HistoryRecord.profileId(type: entry.type, hash: hash)
                try? await PayloadCache.shared.write(profileId: profileId, bytes: payload)
            }
        }
        // Persist BEFORE the metadata PUT, not after. `putClipboard` is the
        // moment the new hash becomes visible to every other client (the
        // main app's `SyncEngine.tick()` GETs `/SyncClipboard.json` 1Hz);
        // if we wrote the hash after the PUT, a concurrent tick would see
        // `server.hash != lastSyncedContentHash` and pull the entry we
        // just pushed back to the device (the "one bounce" loop). The file
        // backend further removes cfprefsd's cross-process cache lag, so
        // by the time `putClipboard` returns, any other process reading
        // `loadLastSyncedHash` sees the new value.
        if let hash = entry.hash, !hash.isEmpty {
            store.saveLastSyncedHash(hash)
        }
        try await client.putClipboard(entry)
        log.info("upload: metadata PUT done, watermark advanced")

        // Surface the push in the shared history log so it shows up in the
        // main app's Home list. The app's SyncEngine won't log it on its own —
        // it sees the watermark we just wrote and treats the server entry as
        // already synced (skipping its own appendHistory).
        store.appendHistory(entry: entry, direction: .pushed)

        // Tell iOS Sharing Suggestions "the user just sent this to this
        // server" so next time the share sheet ranks the server's
        // contact tile higher. Best-effort: failures are swallowed inside.
        await ShareIntentDonation.donateSend(to: server, summary: item.displayName)
    }

    private func build(from item: ShareItem) -> (clipboard: Clipboard, payload: Data?) {
        switch item {
        case .text(let s):
            let (c, p) = Clipboard.publishText(s)
            return (c, p)
        case .image(let bytes, let ext):
            let (c, p) = Clipboard.publishImage(bytes: bytes, ext: ext)
            return (c, p)
        case .file(let name, let bytes):
            let (c, p) = Clipboard.publishFile(name: name, bytes: bytes)
            return (c, p)
        }
    }
}
