import Foundation
import UIKit
import Observation

/// Reads `UIPasteboard.general` and exposes the result as an observable
/// `Clipboard?`. App-target-only (UIKit dependency); not built by SwiftPM.
///
/// Two access tiers, split to keep iOS 16+'s "Allow Paste" prompt off the
/// screen unless the user explicitly asked for content:
///
/// **Free, no-prompt detection** (`pollDetectionIfChanged`, `detection`):
/// reads only `changeCount` + `hasStrings`/`hasImages`/`hasURLs`, none of
/// which paint the privacy banner or fire the modal. Runs on
/// `changedNotification`, `didBecomeActiveNotification`, and once per engine
/// tick when auto-push is OFF (the default). Surfaces the Home one-tap
/// `PasteButton` hint without ever touching content.
///
/// **Content reads** (`read`, `pollIfChanged`, `snapshot`): touch the actual
/// bytes and so CAN prompt. Driven only by (a) the engine tick when the user
/// opted into auto-push, or (b) an explicit push action. The Home
/// `PasteButton` is the prompt-free alternative: it hands us already-read
/// bytes via `consentPush` → `adoptConsentPush`, so the default push path
/// reads nothing here at all.
///
/// Reads text and images. Image read priority is PNG > HEIC > JPEG > GIF
/// (PNG is the screenshot default; HEIC over JPEG matches modern Photos).
/// Bytes are pulled via `data(forPasteboardType:)` — never `pb.image`,
/// which decodes through `UIImage` and breaks the §4.2 hash by changing
/// bytes. File and Group reads from UIPasteboard are not meaningful on
/// iOS (Files-app + Share Extension is the right surface for those).
///
/// DEBUG env hooks (only present so design can be inspected without
/// interactive simulator; not feature flags):
/// - `UC_DEVICE_IMAGE=<fixtureName>` → bypass UIPasteboard, return the
///   named built-in image fixture. Optional `UC_DEVICE_IMAGE_EXT=<ext>`
///   overrides the file extension (default `png`). Image hook takes
///   priority over `UC_DEVICE_TEXT` when both are set.
/// - `UC_DEVICE_TEXT`:
///   - unset           → real `UIPasteboard.general` reads
///   - empty string    → reports `nil` (drives empty-state UI)
///   - any other value → reports `Clipboard.fromText(value)`
///
/// Content reads on iOS 16+ paint the system "pasted from <App>" banner
/// (and the modal for cross-app content). The default push path avoids
/// both by going through SwiftUI's `PasteButton` (see `PastedItemExtractor`
/// + `SyncEngine.consentPush`); the content reads that remain here only run
/// when the user opts into fully-automatic push.
@MainActor
@Observable
final class DevicePasteboardObserver {

    private(set) var current: Clipboard?

    /// A free, no-prompt hint that the device pasteboard holds content the
    /// user could push — derived purely from `changeCount` + the
    /// `hasStrings`/`hasImages`/`hasURLs` accessors, none of which trigger
    /// iOS's "Allow Paste" prompt. `nil` when there's nothing new to push
    /// (empty pasteboard, content we wrote ourselves, or content already
    /// pushed/dismissed). Home renders a one-tap `PasteButton` card off
    /// this; the actual content read only happens when the user taps that
    /// system button (which grants access without a prompt).
    private(set) var detection: PasteboardDetection?

    @ObservationIgnored
    private let envMode: EnvMode

    @ObservationIgnored
    private var observers: [NSObjectProtocol] = []

    @ObservationIgnored
    private let notificationCenter: NotificationCenter

    /// `UIPasteboard.changeCount` recorded immediately after our own write.
    /// `read()` short-circuits when the current changeCount matches this —
    /// the changedNotification firing for our own setData/string assignment
    /// is the echo case we want to ignore (otherwise live-mode image apply
    /// would re-canonicalize basename to `image.<ext>` and mis-flag §4.2
    /// as mismatched). External copies always advance changeCount further,
    /// so they still propagate. `-1` as initial sentinel is safe because
    /// `UIPasteboard.changeCount` is non-negative.
    @ObservationIgnored
    private var lastWriteChangeCount: Int = -1

    /// `UIPasteboard.changeCount` recorded after the last successful read,
    /// regardless of who wrote (us or another app). `pollIfChanged()` uses
    /// this to skip the content-access call (which paints iOS's "pasted
    /// from X" banner) when nothing has actually changed since we last
    /// looked. `-1` sentinel so the very first poll always reads.
    ///
    /// Reason this exists: `UIPasteboard.changedNotification` is unreliable
    /// for cross-app changes — and when iOS shows the "Allow Paste" modal
    /// the read that triggered it returns nil, so `current` is stuck nil
    /// until something re-reads. SyncEngine drives that re-read once per
    /// tick via `pollIfChanged`, which is cheap when nothing changed.
    @ObservationIgnored
    private var lastObservedChangeCount: Int = -1

    /// `UIPasteboard.changeCount` of the most recent content the user
    /// already pushed via the consent path, OR explicitly dismissed. The
    /// free detection poll stays quiet (`detection == nil`) until the
    /// changeCount advances past this — i.e. until the user copies
    /// something genuinely new. `-1` sentinel so a fresh launch surfaces
    /// whatever is already on the pasteboard. Distinct from
    /// `lastWriteChangeCount` (which tracks our own pasteboard *writes*).
    @ObservationIgnored
    private var lastConsumedChangeCount: Int = -1

    /// Content hash of the most recent value we wrote to `UIPasteboard`,
    /// uppercase. Secondary echo guard: when changeCount drifts past
    /// `lastWriteChangeCount` (an unrelated process bumped the pasteboard,
    /// iOS posted an extra notification, etc.) we re-snapshot — but if
    /// the snapshot's §4.1/§4.2 hash matches `lastWrittenContentHash`,
    /// the bytes are still ours and we suppress the read. Without this,
    /// the apply path can re-snapshot to a Clipboard with a slightly
    /// different §4.2 basename (because `imageUTIPriority` canonicalizes
    /// to `image.<ext>`), and that re-snapshotted hash drives a spurious
    /// push that pings back as the next pull — the apply↔push pong.
    @ObservationIgnored
    private var lastWrittenContentHash: String?

    /// Gate that defers the first live `UIPasteboard.general` access until
    /// the UI explicitly calls `activate()`. Without this, the observer
    /// reads at init (and again on `didBecomeActiveNotification` during the
    /// initial activation), which fires iOS 16+'s "Allow Paste" prompt
    /// before the user has any visual context — they see the system alert
    /// over a splash / Setup flow they're still trying to read. Env-driven
    /// modes never touch `UIPasteboard.general`, so they auto-activate in
    /// init and previews / screenshot recipes keep working unchanged.
    @ObservationIgnored
    private var isActive: Bool = false

    init(
        notificationCenter: NotificationCenter = .default,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) {
        self.notificationCenter = notificationCenter
        self.envMode = EnvMode(environment: environment)
        subscribe()
        if case .live = envMode {
            // Defer first read to activate() — see `isActive`.
        } else {
            isActive = true
            read()
        }
    }

    /// Permit pasteboard access. Call once the home tab is on screen.
    ///
    /// Deliberately does **not** read content — it only runs the free
    /// detection poll. Reading content here is exactly what fired the iOS
    /// "Allow Paste" prompt on cold launch / first foreground. Content is
    /// now read only on an explicit user gesture (the Home `PasteButton`)
    /// or, when the user has opted into auto-push, by the engine tick via
    /// `pollIfChanged()`. Idempotent.
    func activate() {
        isActive = true
        pollDetectionIfChanged()
    }

    /// Free, no-prompt poll. Reads only `changeCount` and the
    /// `hasStrings`/`hasImages`/`hasURLs` accessors — none of which paint
    /// the privacy banner or fire the "Allow Paste" modal — and updates
    /// `detection`. Called on foreground / `changedNotification` and once
    /// per engine tick when auto-push is OFF, so a fresh local copy
    /// surfaces the Home push hint within one cadence without ever reading
    /// content the user didn't ask us to read.
    func pollDetectionIfChanged() {
        guard isActive, case .live = envMode else { return }
        let pb = UIPasteboard.general
        let cc = pb.changeCount
        // Ours (just applied a server entry) or already pushed/dismissed →
        // nothing for the user to act on.
        if cc == lastWriteChangeCount || cc == lastConsumedChangeCount {
            if detection != nil { detection = nil }
            return
        }
        let kind: PasteboardDetection.Kind?
        if pb.hasImages      { kind = .image }
        else if pb.hasURLs   { kind = .url }
        else if pb.hasStrings { kind = .text }
        else                 { kind = nil }
        guard let kind else {
            if detection != nil { detection = nil }
            return
        }
        let next = PasteboardDetection(kind: kind, changeCount: cc)
        if detection != next { detection = next }
    }

    /// Record that the content currently on the pasteboard was just pushed
    /// to the server via the consent path. Surfaces it as `current` (so the
    /// Home card/list reflects it), marks its changeCount consumed so the
    /// detection hint clears and won't re-fire until the next external copy,
    /// and stashes the content hash so a later content read recognizes it as
    /// already-synced. No pasteboard write happens — the bytes are already
    /// there; this is bookkeeping only.
    func adoptConsentPush(_ clipboard: Clipboard) {
        current = clipboard
        detection = nil
        guard case .live = envMode else { return }
        let cc = UIPasteboard.general.changeCount
        lastConsumedChangeCount = cc
        lastObservedChangeCount = cc
        lastWrittenContentHash = clipboard.hash?.uppercased()
    }

    /// Cheap poll: read `UIPasteboard.general.changeCount` (free, no
    /// privacy banner) and only call `read()` when it has advanced past
    /// both our own last write and our last observed value. Called from
    /// `SyncEngine.tick()` at 1Hz so cross-app pasteboard changes — which
    /// `UIPasteboard.changedNotification` does not reliably deliver, and
    /// which `didBecomeActive` only covers on the foreground transition —
    /// get picked up within one tick. Also recovers the case where the
    /// iOS "Allow Paste" modal swallowed the first read after foreground
    /// (the triggering call returned nil; tapping Allow doesn't re-deliver
    /// the bytes — but the next tick will see the same changeCount, read
    /// once more silently, and surface the content).
    ///
    /// Env-driven modes are deterministic and have no changeCount, so this
    /// is a no-op for them — their `current` is set in init and at
    /// `activate()`.
    func pollIfChanged() {
        guard isActive, case .live = envMode else { return }
        let cc = UIPasteboard.general.changeCount
        if cc == lastWriteChangeCount { return }
        if cc == lastObservedChangeCount { return }
        read()
    }

    /// Write `text` to `UIPasteboard.general`. We adopt `current` to the
    /// publish-shape clipboard immediately (don't wait for the change
    /// notification) and capture changeCount so the echo notification is
    /// ignored. Two-layer defense: even if changeCount tracking races, the
    /// adopted `current` already shows the synced state. Under env hooks,
    /// we skip the system call to keep simctl recipes hermetic.
    func write(text: String) {
        switch envMode {
        case .live:
            UIPasteboard.general.string = text
            lastWriteChangeCount = UIPasteboard.general.changeCount
            let adopted = Clipboard.publishText(text).clipboard
            lastWrittenContentHash = adopted.hash?.uppercased()
            lastConsumedChangeCount = UIPasteboard.general.changeCount
            current = adopted
        case .forceNil, .forceText, .forceImage:
            let adopted = Clipboard.fromText(text)
            lastWrittenContentHash = adopted.hash?.uppercased()
            current = adopted
        }
        // We just put this on the pasteboard ourselves — it's not something
        // the user needs to be nudged to push.
        detection = nil
    }

    /// Write `data` to `UIPasteboard.general` under a specific UTI. Used
    /// for image apply (write server image bytes back to the device
    /// pasteboard so they can be pasted into another app).
    ///
    /// `originalName`, when non-nil, is the dataName from the server entry
    /// being applied; it's adopted for display (`text`/`dataName`) only.
    /// The hash is raw-bytes SHA-256 — basename does not participate, so
    /// the adopted hash, a later re-snapshot's hash, and the server's hash
    /// all agree regardless of how UIPasteboard canonicalizes the name.
    /// In live mode we ALSO call `setData` and capture `changeCount`; the
    /// echo notification that fires next is ignored by `read()` because
    /// changeCount matches.
    func write(data: Data, uti: String, originalName: String? = nil) {
        let name = originalName ?? "image.\(Self.ext(forUTI: uti))"
        let hash = Clipboard.computeBytesHash(data)
        let adopted = Clipboard(
            type: .image,
            hash: hash,
            text: name,
            hasData: true,
            dataName: name,
            size: data.count
        )
        switch envMode {
        case .live:
            UIPasteboard.general.setData(data, forPasteboardType: uti)
            lastWriteChangeCount = UIPasteboard.general.changeCount
            lastConsumedChangeCount = lastWriteChangeCount
        case .forceNil, .forceText, .forceImage:
            break
        }
        lastWrittenContentHash = hash.uppercased()
        current = adopted
        detection = nil
    }

    /// Re-read the pasteboard (or env override) into `current`. Idempotent;
    /// safe to call from notifications and explicit UI triggers. Discards
    /// payload bytes — the UI doesn't need them. Push uses `snapshot()`
    /// instead so it gets fresh bytes at push time.
    ///
    /// Echo guard: when the live `changeCount` equals the one we recorded
    /// after our own most recent write, this is the notification firing
    /// for that write and `current` is already the adopted entry — bail
    /// out so we don't re-canonicalize basename. External copies advance
    /// changeCount further and fall through to the fresh read.
    ///
    /// Secondary hash-equivalence echo guard: when changeCount has drifted
    /// past `lastWriteChangeCount` (an unrelated process bumped the
    /// pasteboard, iOS posted an extra notification, …) we re-snapshot
    /// — but if the snapshot's hash matches `lastWrittenContentHash`,
    /// the bytes are still ours and any further mutation of `current`
    /// would discard the server's `dataName` binding we just adopted.
    /// Keep `current` as-is so the SyncEngine's apply-vs-push gate stays
    /// stable.
    func read() {
        guard isActive else { return }
        if case .live = envMode,
           UIPasteboard.general.changeCount == lastWriteChangeCount {
            return
        }
        guard let snap = snapshot()?.clipboard else {
            // Likely the iOS "Allow Paste" modal swallowed this read — the
            // bytes are not actually nil on the device, we just don't have
            // permission yet. Deliberately do NOT advance
            // `lastObservedChangeCount` here so the next `pollIfChanged`
            // re-reads at the same changeCount once permission lands.
            current = nil
            return
        }
        if case .live = envMode {
            lastObservedChangeCount = UIPasteboard.general.changeCount
        }
        if let written = lastWrittenContentHash,
           let snapHash = snap.hash?.uppercased(),
           written == snapHash {
            // Echo (delayed / out-of-order notification, or external
            // changeCount bump). Don't overwrite `current` — it still
            // carries the server-side basename binding the SyncEngine
            // needs for its dedup.
            return
        }
        current = snap
    }

    /// Bytes-fresh read. Returns the current pasteboard contents as a
    /// `Clipboard` plus the raw bytes (when applicable). Push reads at
    /// push time via this API rather than relying on the cached `current`,
    /// which closes the race where the user copies a new item between an
    /// observer notification firing and the push action firing.
    func snapshot() -> DeviceClipboardSnapshot? {
        guard isActive else { return nil }
        switch envMode {
        case .live:
            return liveSnapshot()
        case .forceNil:
            return nil
        case .forceText(let s):
            let (clip, payload) = Clipboard.publishText(s)
            return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
        case .forceImage(let data, let ext):
            let (clip, payload) = Clipboard.publishImage(bytes: data, ext: ext)
            return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
        }
    }

    private func liveSnapshot() -> DeviceClipboardSnapshot? {
        let pb = UIPasteboard.general
        for (uti, ext) in Self.imageUTIPriority {
            if let data = pb.data(forPasteboardType: uti), !data.isEmpty {
                let (clip, payload) = Clipboard.publishImage(bytes: data, ext: ext)
                return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
            }
        }
        if let s = pb.string, !s.isEmpty {
            let (clip, payload) = Clipboard.publishText(s)
            return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
        }
        return nil
    }

    /// PNG first (screenshot default + lossless), HEIC over JPEG (modern
    /// Photos), GIF last (rare).
    private static let imageUTIPriority: [(uti: String, ext: String)] = [
        ("public.png", "png"),
        ("public.heic", "heic"),
        ("public.jpeg", "jpg"),
        ("com.compuserve.gif", "gif"),
    ]

    private static func ext(forUTI uti: String) -> String {
        switch uti {
        case "public.png":         return "png"
        case "public.heic", "public.heif": return "heic"
        case "public.jpeg":        return "jpg"
        case "com.compuserve.gif": return "gif"
        default:                   return "bin"
        }
    }

    private func subscribe() {
        // Both notifications now drive the FREE detection poll, never a
        // content read. Reading content on `changedNotification` /
        // `didBecomeActive` is what fired the "Allow Paste" prompt the
        // moment the user copied something elsewhere and returned. When the
        // user opts into auto-push, the engine tick does the content read
        // via `pollIfChanged()` instead.
        observers.append(
            notificationCenter.addObserver(
                forName: UIPasteboard.changedNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in self?.pollDetectionIfChanged() }
            }
        )
        observers.append(
            notificationCenter.addObserver(
                forName: UIApplication.didBecomeActiveNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in self?.pollDetectionIfChanged() }
            }
        )
    }

    private enum EnvMode {
        case live
        case forceNil
        case forceText(String)
        case forceImage(Data, String)

        init(environment: [String: String]) {
            if let imgKey = environment["UC_DEVICE_IMAGE"],
               let bytes = ImageFixtures.bytes(for: imgKey) {
                let ext = environment["UC_DEVICE_IMAGE_EXT"] ?? "png"
                self = .forceImage(bytes, ext)
                return
            }
            switch environment["UC_DEVICE_TEXT"] {
            case .none:                        self = .live
            case .some(let s) where s.isEmpty: self = .forceNil
            case .some(let s):                 self = .forceText(s)
            }
        }
    }
}

// `DeviceClipboardSnapshot` moved to `Shared/Models/DeviceClipboardSnapshot.swift`
// so the Share + Widget extensions can reference it without this UIKit-bound file.

/// A no-prompt detection result: the *kind* of content sitting on the
/// device pasteboard and the `changeCount` it was seen at. Derived only
/// from `UIPasteboard.hasImages`/`hasURLs`/`hasStrings` — never from a
/// content read — so building one of these never triggers iOS's "Allow
/// Paste" prompt. `changeCount` lets the observer dedup "already pushed /
/// dismissed" without re-reading.
struct PasteboardDetection: Equatable {
    enum Kind: Equatable { case text, url, image }
    let kind: Kind
    let changeCount: Int
}

/// Built-in image fixtures keyed by a short name. Same `red8x8` PNG the
/// simctl stub serves so device-side `publishImage` and stub-side
/// `_bytes_hash` produce identical §4.2 hashes — cross-recipe
/// consistency without copy-paste.
private enum ImageFixtures {
    static let red8x8PNG = Data(base64Encoded:
        "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAIAQMAAAD+wSzIAAAABGdBTUEAALGP" +
        "C/xhBQAAAAFzUkdCAK7OHOkAAAAGUExURf8AAP///8jJRKEAAAAOSURBVAjX" +
        "Y/jPwMDAAAAEAQEAQYxqNwAAAABJRU5ErkJggg=="
    )!

    static func bytes(for name: String) -> Data? {
        switch name {
        case "red8x8": return red8x8PNG
        default: return nil
        }
    }
}
