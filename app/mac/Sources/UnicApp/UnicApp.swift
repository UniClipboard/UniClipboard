import SwiftUI
import AppKit

@main struct UnicApp: App {
    @ObservedObject private var library = Library.shared

    init() {
        HotKey.register { MainActor.assumeIsolated { QuickPanel.shared.toggle() } }
    }

    var body: some Scene {
        Window("Unic", id: "library") {
            LibraryWindow().environmentObject(library)
                .frame(minWidth: 900, minHeight: 560)
                .onAppear {
                    NSApp.setActivationPolicy(.regular)
                    NSApp.activate(ignoringOtherApps: true)
                    #if DEBUG
                    Snapshot.scheduleIfRequested()
                    #endif
                }
        }
        .defaultSize(width: 1240, height: 780)
        .windowToolbarStyle(.unifiedCompact(showsTitle: true))
        .commands {
            CommandGroup(after: .newItem) {
                Button("Quick Paste (⌥⌘V)") { QuickPanel.shared.show() }
            }
        }

        Settings { SettingsView().environmentObject(library) }

        MenuBarExtra("Unic", systemImage: "doc.on.clipboard") {
            MenuPanel().environmentObject(library)
        }
        .menuBarExtraStyle(.window)
    }
}
