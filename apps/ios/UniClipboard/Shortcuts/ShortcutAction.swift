import Foundation
import Observation

/// Home Screen quick-action tile that fires when the user long-presses
/// the UniClipboard icon on the Springboard. The raw value is the wire
/// identifier UIKit hands back via `UIApplicationShortcutItem.type`, so
/// it must stay stable across releases — changing it orphans tiles that
/// iOS has already cached for users who installed an earlier build.
enum ShortcutAction: String, Hashable, Sendable {
    case push = "app.uniclipboard.UniClipboard.shortcut.push"
    case pull = "app.uniclipboard.UniClipboard.shortcut.pull"
}

/// Single-slot bridge between the UIKit `AppDelegate` callbacks and the
/// SwiftUI view tree. The delegate writes a parsed `ShortcutAction` here
/// from any callback path — cold-launch `didFinishLaunchingWithOptions`
/// or runtime `performActionFor` — and `ContentView` observes the
/// value and dispatches into `AppViewModel.runShortcut`.
///
/// A singleton (rather than @State on the App struct) because the
/// delegate fires *before* the SwiftUI view tree has mounted on cold
/// launch; the value has to survive that gap so the consumer can drain
/// it on first appear. `@Observable` lets SwiftUI re-render `ContentView`
/// when a runtime invocation lands while the app is already on screen.
@MainActor
@Observable
final class ShortcutInbox {
    static let shared = ShortcutInbox()
    var pending: ShortcutAction?
    private init() {}
}
