import AppIntents

/// Surfaces `SendClipboardIntent` and `ReceiveClipboardIntent` to the
/// system Shortcuts app, Siri, Spotlight, the Action Button, and the
/// Control Center widget gallery automatically — no user setup beyond
/// "add to Home Screen" / "add to Control Center" needed.
///
/// Once these `AppShortcut`s register, the user can:
/// - Open the Shortcuts app → UniClipboard → drag either shortcut to a
///   Home Screen page. That tile triggers the intent directly with
///   `openAppWhenRun = false`, so the device never opens UniClipboard.
/// - Say "Hey Siri, 用 UniClipboard 发送 / 接收".
/// - Bind to the Action Button (iPhone 15 Pro+) or a Control Center
///   slot for one-tap background execution.
///
/// Every phrase MUST contain `\(.applicationName)` (the framework rejects
/// the shortcut at build time otherwise) so Siri can distinguish our
/// intent from other apps' similarly-named ones.
struct UniClipboardAppShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: SendClipboardIntent(),
            phrases: [
                "用 \(.applicationName) 发送",
                "用 \(.applicationName) 发送剪贴板",
                "\(.applicationName) 发送剪贴板",
                "Send clipboard with \(.applicationName)",
            ],
            shortTitle: "发送",
            systemImageName: "square.and.arrow.up"
        )
        AppShortcut(
            intent: ReceiveClipboardIntent(),
            phrases: [
                "用 \(.applicationName) 接收",
                "用 \(.applicationName) 接收剪贴板",
                "\(.applicationName) 接收剪贴板",
                "Receive clipboard with \(.applicationName)",
            ],
            shortTitle: "接收",
            systemImageName: "square.and.arrow.down"
        )
    }
}
