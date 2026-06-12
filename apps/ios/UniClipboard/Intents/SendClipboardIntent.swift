import AppIntents
import Foundation
import UIKit
import UniformTypeIdentifiers

/// "发送" App Intent — pushes content to a SyncClipboard server.
/// `openAppWhenRun = false` so it runs from the Shortcuts app, a
/// Home-Screen icon made from it, Siri, or the Action Button without the
/// main app window appearing.
///
/// Three parameters make this usable as a real Shortcuts building block —
/// and fix the two long-standing failure modes of the old "read the system
/// pasteboard in the background" design:
///
/// - `server`: the destination. Picked in the Shortcuts editor, so a user
///   can branch on Wi-Fi with the system "If" action and route to the right
///   backend. Nil (the bare Siri / AppShortcut phrase) falls back to
///   `activeConfig`, matching the old behavior.
/// - `text` / `file`: the content to send. Feeding content in as a
///   parameter sidesteps iOS's background `UIPasteboard` restriction — a
///   background intent often reads an empty pasteboard, which used to
///   surface as a bogus "剪贴板为空". When both are nil we still fall back to
///   the pasteboard so the legacy foreground path keeps working.
///
/// Mirrors `ShareUploader`'s §3.5 file-first sequence and the
/// `lastSyncedContentHash` watermark write so the main app's `SyncEngine`
/// doesn't echo the just-pushed entry back to the device pasteboard.
struct SendClipboardIntent: AppIntent {
    static var title: LocalizedStringResource = "发送剪贴板"
    static var description = IntentDescription(
        "把内容发送到 UniClipboard 服务器,供其他设备接收。可在快捷指令中指定目标服务器,以及要发送的文本或文件;都不指定时回退读取本机剪贴板。"
    )

    /// Run silently. Foregrounding the app for a push the user already
    /// understands is just noise on the Springboard.
    static var openAppWhenRun: Bool = false

    @Parameter(title: "服务器")
    var server: ServerEntity?

    @Parameter(title: "文本")
    var text: String?

    @Parameter(title: "文件")
    var file: IntentFile?

    static var parameterSummary: some ParameterSummary {
        Summary("发送到 \(\.$server)") {
            \.$text
            \.$file
        }
    }

    /// `@MainActor` because the project sets
    /// `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor`, which makes
    /// `SettingsStore`, `ServerConfigList.activeConfig`, and
    /// `SyncClipboardClient.init` all MainActor-isolated in this target.
    /// The body is `await`-heavy and hops to background work via URLSession
    /// internally — keeping perform on MainActor doesn't pin the network.
    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let store = SettingsStore()
        let servers = store.loadServers()
        let appSettings = store.loadAppSettings()
        guard let target = ServerEntity.resolveConfig(server, in: servers, store: store) else {
            return .result(dialog: "请先在 UniClipboard 中添加一台服务器")
        }

        guard let (clip, payload) = resolvePayload() else {
            return .result(dialog: "没有可发送的内容 — 请在快捷指令里提供文本或文件,或先复制一些内容")
        }

        do {
            let client = try SyncClipboardClient(
                server: target,
                trustInsecureCert: appSettings.trustInsecureCert
            )
            // §3.5: payload bytes first, metadata second. Without this order
            // the server can briefly serve a metadata pointer to a missing file.
            if clip.hasData, let payload, let name = clip.dataName {
                try await client.putFile(name: name, body: payload)
            }
            // Watermark BEFORE the metadata PUT (same reasoning as
            // ShareUploader): putClipboard is when the new hash becomes
            // visible to the 1 Hz SyncEngine tick; writing the watermark
            // after would let a concurrent tick bounce the entry back.
            if let hash = clip.hash, !hash.isEmpty {
                store.saveLastSyncedHash(hash)
            }
            try await client.putClipboard(clip)
            // Best-effort Sharing-Suggestions donation, same as ShareUploader.
            await ShareIntentDonation.donateSend(to: target, clipboard: clip)
            return .result(dialog: IntentDialog("已发送到 \(target.displayLabel)"))
        } catch let e as SyncError {
            return .result(dialog: IntentDialog(stringLiteral: "发送失败:\(Self.errorMessage(e))"))
        }
    }

    /// Decide WHAT to send, in priority order:
    /// 1. an explicit `file` parameter (image / file / text document),
    /// 2. an explicit `text` parameter,
    /// 3. the device pasteboard (legacy fallback — only reliable in the
    ///    foreground; the parameters exist precisely so a Shortcut can feed
    ///    content WITHOUT depending on background pasteboard access).
    private func resolvePayload() -> (clipboard: Clipboard, payload: Data?)? {
        if let file {
            return Self.publish(file: file)
        }
        if let text, !text.isEmpty {
            return Clipboard.publishText(text)
        }
        return Self.pasteboardSnapshot()
    }

    /// Map an `IntentFile` onto the right `Clipboard.publish*` builder by
    /// inspecting its declared `type`: text documents go through the text
    /// path (so short text stays inline per §3.4), images through the image
    /// path, everything else is treated as an opaque file.
    private static func publish(file: IntentFile) -> (clipboard: Clipboard, payload: Data?) {
        let data = file.data
        let filename = file.filename
        if let type = file.type, type.conforms(to: .image) {
            let ext = type.preferredFilenameExtension
                ?? (filename as NSString).pathExtension
            let (c, p) = Clipboard.publishImage(bytes: data, ext: ext.isEmpty ? "png" : ext)
            return (c, p)
        }
        if let type = file.type, type.conforms(to: .text) {
            return Clipboard.publishText(String(decoding: data, as: UTF8.self))
        }
        let (c, p) = Clipboard.publishFile(name: filename, bytes: data)
        return (c, p)
    }

    /// Legacy pasteboard read. Mirrors `DevicePasteboardObserver`'s UTI
    /// priority (PNG > HEIC > JPEG > GIF > text) so a screenshot pushed via
    /// the intent hashes identically to one pushed via the auto-sync tick.
    private static func pasteboardSnapshot() -> (clipboard: Clipboard, payload: Data?)? {
        let pb = UIPasteboard.general
        let imageUTIs: [(uti: String, ext: String)] = [
            ("public.png", "png"),
            ("public.heic", "heic"),
            ("public.jpeg", "jpg"),
            ("com.compuserve.gif", "gif"),
        ]
        for (uti, ext) in imageUTIs {
            if let data = pb.data(forPasteboardType: uti), !data.isEmpty {
                let (clip, payload) = Clipboard.publishImage(bytes: data, ext: ext)
                return (clip, payload)
            }
        }
        if let s = pb.string, !s.isEmpty {
            return Clipboard.publishText(s)
        }
        return nil
    }

    /// User-facing message for `SyncError`. Shared with `ReceiveClipboardIntent`.
    static func errorMessage(_ err: SyncError) -> String {
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
