import UIKit

/// Minimal `UIApplicationDelegate` whose only job is to surface Home
/// Screen quick-action invocations into `ShortcutInbox`. SwiftUI's `App`
/// lifecycle doesn't expose `UIApplicationShortcutItem` directly, so the
/// `@UIApplicationDelegateAdaptor` plumbing in `UniClipboardApp` routes
/// UIKit's two callback paths through this class.
///
/// We rely on the documented fallback: when a scene-based app (which
/// SwiftUI `WindowGroup` is under the hood) provides no `UISceneDelegate`
/// implementation of `windowScene(_:performActionFor:)`, UIKit bubbles
/// the call up to `application(_:performActionFor:)` on the app delegate.
/// Same for cold-launch — `launchOptions[.shortcutItem]` carries the
/// invocation. This lets us avoid installing a custom scene delegate
/// (which would replace SwiftUI's internal one and break the view tree).
final class AppDelegate: NSObject, UIApplicationDelegate {
    /// Cold-launch: the user tapped a quick action while the app was not
    /// running. SwiftUI mounts shortly after this returns; the inbox
    /// value survives the gap so `ContentView.task` can drain it.
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        if let item = launchOptions?[.shortcutItem] as? UIApplicationShortcutItem,
           let action = ShortcutAction(rawValue: item.type) {
            ShortcutInbox.shared.pending = action
        }
        return true
    }

    /// Runtime: the app is alive (foreground or background) and the user
    /// invoked the quick action. `completionHandler(true)` is unconditional
    /// — the actual work runs asynchronously through `runShortcut`, which
    /// has its own error surfaces on `AppViewModel` (`pushError`,
    /// `refreshError`, `applyError`). Reporting `false` here would just
    /// dim the tile in iOS's UI without helping the user.
    func application(
        _ application: UIApplication,
        performActionFor shortcutItem: UIApplicationShortcutItem,
        completionHandler: @escaping (Bool) -> Void
    ) {
        if let action = ShortcutAction(rawValue: shortcutItem.type) {
            ShortcutInbox.shared.pending = action
        }
        completionHandler(true)
    }
}
