import AppIntents
import Foundation
import UIKit

/// "接收" App Intent — pulls the server's latest entry and returns it as the
/// shortcut's output, optionally also writing it onto `UIPasteboard.general`.
/// `openAppWhenRun = false` so it runs from a Home-Screen icon, Siri, or the
/// Action Button without the UniClipboard window ever appearing.
///
/// Two design changes over the old version:
///
/// - `server`: the source. Picked in the Shortcuts editor so the action can
///   be routed per-network (system "If Wi-Fi network is …" → 接收 from the
///   matching server). Nil falls back to `activeConfig`, matching the old
///   bare-phrase behavior.
/// - Returns the text as a `ReturnsValue<String>` so a Shortcut can chain it
///   into any follow-up action without depending on the system pasteboard.
///   `copyToDevice` (default on) still writes the pasteboard for the
///   "接收即可粘贴" muscle memory; turning it off makes this a pure read.
///
/// Pull/verify logic mirrors `AppViewModel.applyServerToDevice()`'s switch on
/// `Clipboard.Kind` (§2.4 getFile → §4 verify). `.file` / `.group` have no
/// meaningful single text value, so they only report via dialog.
struct ReceiveClipboardIntent: AppIntent {
    static var title: LocalizedStringResource = "接收剪贴板"
    static var description = IntentDescription(
        "从 UniClipboard 服务器拉取最新内容并作为快捷指令的输出返回;默认同时写入本机剪贴板。可指定来源服务器。"
    )

    static var openAppWhenRun: Bool = false

    @Parameter(title: "服务器")
    var server: ServerEntity?

    @Parameter(title: "写入本机剪贴板", default: true)
    var copyToDevice: Bool

    static var parameterSummary: some ParameterSummary {
        Summary("从 \(\.$server) 接收") {
            \.$copyToDevice
        }
    }

    /// See `SendClipboardIntent.perform` for the `@MainActor` rationale.
    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog {
        let store = SettingsStore()
        let servers = store.loadServers()
        let appSettings = store.loadAppSettings()
        guard let target = ServerEntity.resolveConfig(server, in: servers, store: store) else {
            return .result(value: "", dialog: "请先在 UniClipboard 中添加一台服务器")
        }

        do {
            let client = try SyncClipboardClient(
                server: target,
                trustInsecureCert: appSettings.trustInsecureCert
            )
            let entry = try await client.getClipboard()

            switch entry.type {
            case .text:
                let value: String
                if entry.hasData, let name = entry.dataName {
                    let bytes = try await client.getFile(name: name)
                    try Self.verify(bytes: bytes, against: entry)
                    value = String(decoding: bytes, as: UTF8.self)
                } else {
                    value = entry.text
                }
                if copyToDevice {
                    UIPasteboard.general.string = value
                    Self.saveWatermark(entry, store: store)
                }
                return .result(
                    value: value,
                    dialog: copyToDevice ? "已接收并写入本机剪贴板" : "已接收最新内容"
                )

            case .image:
                guard entry.hasData, let name = entry.dataName else {
                    return .result(value: "", dialog: "服务器最新内容没有图像数据")
                }
                let bytes = try await client.getFile(name: name)
                try Self.verify(bytes: bytes, against: entry)
                if copyToDevice {
                    UIPasteboard.general.setData(bytes, forPasteboardType: Self.utiForDataName(name))
                    Self.saveWatermark(entry, store: store)
                }
                // Image bytes can't be a String return value; hand back the
                // name so a Shortcut at least has a label to branch on.
                return .result(
                    value: name,
                    dialog: copyToDevice ? "已接收图像并写入本机剪贴板" : "已接收图像(\(name))"
                )

            case .file, .group:
                return .result(value: "", dialog: "服务器最新内容是文件或多类型组合,无法直接作为文本接收")
            }
        } catch let e as SyncError where e.kind == .notFound {
            return .result(value: "", dialog: "服务器上还没有任何内容")
        } catch let e as SyncError {
            return .result(value: "", dialog: IntentDialog(stringLiteral: "接收失败:\(SendClipboardIntent.errorMessage(e))"))
        }
    }

    /// Only watermark when we actually wrote the device pasteboard — the
    /// watermark means "device content == this hash", which lets the
    /// SyncEngine skip a push-back. Writing it when `copyToDevice` is off
    /// would lie about the device's state and could suppress a legitimate
    /// later apply.
    private static func saveWatermark(_ entry: Clipboard, store: SettingsStore) {
        if let hash = entry.hash, !hash.isEmpty {
            store.saveLastSyncedHash(hash)
        }
    }

    /// §4.4 verify: SHA-256 over raw bytes for all types. Mirrors
    /// `AppViewModel.verify(bytes:against:)`.
    private static func verify(bytes: Data, against entry: Clipboard) throws {
        if entry.type == .group { return }
        let actual = Clipboard.computeBytesHash(bytes)
        guard Clipboard.hashMatches(expected: entry.hash, actual: actual) else {
            throw SyncError(
                kind: .hashMismatch,
                underlying: "expected=\(entry.hash ?? "<nil>") actual=\(actual)"
            )
        }
    }

    /// Mirror of `AppViewModel.utiForDataName(_:)`.
    private static func utiForDataName(_ name: String) -> String {
        let ext = (name as NSString).pathExtension.lowercased()
        switch ext {
        case "png":          return "public.png"
        case "heic", "heif": return "public.heic"
        case "jpg", "jpeg":  return "public.jpeg"
        case "gif":          return "com.compuserve.gif"
        default:             return "public.data"
        }
    }
}
