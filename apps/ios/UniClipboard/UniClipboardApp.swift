//
//  UniClipboardApp.swift
//  UniClipboard
//
//  Created by mark on 2026/5/9.
//

import SwiftUI

@main
struct UniClipboardApp: App {
    // `AppDelegate` exists solely to bridge UIKit's `UIApplicationShortcutItem`
    // callbacks into `ShortcutInbox`; SwiftUI's `App` lifecycle has no
    // first-class shortcut API. The shortcut *tiles* themselves are
    // statically declared in Info.plist (`UIApplicationShortcutItems`,
    // injected via the project's PlistBuddy build phase) — dynamic
    // registration via `UIApplication.shared.shortcutItems` is unreliable
    // on real devices because the Springboard caches the LaunchServices
    // snapshot, so a tile that only exists at runtime may never appear.
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    @State private var vm = AppViewModel()

    init() {
        // Must run before any other code so the crash handler covers the
        // whole launch (including AppViewModel/SyncEngine construction).
        SentryBootstrap.start()
    }

    var body: some Scene {
        WindowGroup {
            ContentView(vm: vm)
        }
    }
}
