import SwiftUI

struct ContentView: View {
    @Bindable var vm: AppViewModel

    @State private var showingSettings = false

    /// Path for the Settings NavigationStack. Initial value lets a
    /// `UC_SETTINGS_ROUTE=servers` env hook deep-link into the Servers
    /// list (or `servers/edit/<idx>` to land on the editor) — needed
    /// because simctl can't synthesize taps for screenshot recipes.
    @State private var settingsPath: [SettingsRoute] = SettingsRoute.initialPath()

    /// Set once the user finishes/skips the first-run walkthrough this session.
    /// Lets `UC_ONBOARDING=1` still force the walkthrough on cold launch (for
    /// screenshots) while letting the user actually leave it — otherwise the env
    /// gate re-shows the walkthrough forever and SetupFlow (QR pairing) is never
    /// reachable.
    @State private var onboardingDone = false

    /// Drives the post-pairing "解锁更多" enhancements carousel — auto-presented
    /// once right after the first-run pairing completes, and re-presentable via
    /// the `UC_ONBOARDING_ENHANCE=1` env hook for screenshots. The persisted
    /// `enhancementsPromptShown` flag is the "only once, ever" guard; this
    /// session flag is what actually mounts the sheet.
    @State private var showEnhancements = false

    @Environment(\.scenePhase) private var scenePhase

    /// First-run onboarding gate. Shows the walkthrough only on a truly fresh
    /// install — no servers configured AND onboarding never completed. The
    /// `UC_ONBOARDING=1` env hook forces it on regardless so simctl recipes
    /// can screenshot the flow even after it has been completed once.
    private var showOnboarding: Bool {
        if onboardingDone { return false }
        if ProcessInfo.processInfo.environment["UC_ONBOARDING"] == "1" { return true }
        return !vm.appSettings.onboardingShown && vm.servers.configs.isEmpty
    }

    /// Raise the post-pairing enhancements carousel once, just after the
    /// first-run pairing. Deferred a beat so the SetupFlow `fullScreenCover`
    /// finishes dismissing and `mainContent` mounts before we present the sheet —
    /// stacking a present on the same runloop tick as the cover dismiss + the
    /// onboarding→tabs branch switch swallows it. Marks the persisted flag at
    /// present-time so it never pops again.
    private func presentEnhancementsIfDue() {
        guard !vm.appSettings.enhancementsPromptShown else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            vm.appSettings.enhancementsPromptShown = true
            showEnhancements = true
        }
    }

    var body: some View {
        // Read inside body so the Observation framework registers
        // `ContentView` as an observer of `ShortcutInbox.shared.pending`
        // — without this read, `.onChange` would never fire when the
        // delegate writes a runtime shortcut invocation.
        let pendingShortcut = ShortcutInbox.shared.pending

        return Group {
            if showOnboarding {
                OnboardingView(mode: .firstRun) {
                    vm.completeOnboarding()
                    onboardingDone = true
                }
            } else {
                mainContent
            }
        }
        // Apply the user's appearance preference. `.system` resolves to
        // nil so SwiftUI releases the override and follows iOS. The
        // share-extension process reads the same setting in ShareRootView.
        .preferredColorScheme(vm.appSettings.appearance.colorScheme)
        // `.onOpenURL` fires on the root regardless of which branch is on
        // screen, including while a sheet/modal is up. The dispatcher in
        // AppViewModel stages the parsed payload (or error) into
        // observable state, and the relevant branch consumes it:
        //   • Setup branch reads `pendingImport` via its own `.task(id:)`
        //     and seeds the prefilled form step.
        //   • Tabs branch shows `ConnectImportSheet` below.
        .onOpenURL { vm.handleIncomingURL($0) }
        // Runtime path: app already on screen when a quick action fires.
        // The delegate writes to the inbox, body re-renders, onChange
        // sees the transition and dispatches. `initial: true` covers the
        // cold-launch case where the delegate wrote *before* SwiftUI
        // mounted, so `pendingShortcut` is non-nil on the very first
        // body evaluation and no nil→value transition will occur.
        .onChange(of: pendingShortcut, initial: true) { _, action in
            guard let action else { return }
            ShortcutInbox.shared.pending = nil
            Task { await vm.runShortcut(action) }
        }
        // Confirmation sheet — only meaningful when configs is non-empty.
        // While in Setup, the SetupFlowView's `.task(id:)` consumes
        // `pendingImport` first, so the sheet binding never fires.
        .sheet(item: $vm.pendingImport) { payload in
            ConnectImportSheet(
                payload: payload,
                onConfirm: { vm.acceptPendingImport(payload) },
                onCancel: { vm.pendingImport = nil }
            )
            .presentationDetents([.medium, .large])
        }
        .alert(
            "无法识别该二维码",
            isPresented: Binding(
                get: { vm.importError != nil },
                set: { if !$0 { vm.importError = nil } }
            ),
            presenting: vm.importError
        ) { _ in
            Button("好") { vm.importError = nil }
        } message: { err in
            Text(connectURIErrorMessage(err))
        }
    }

    private var mainContent: some View {
        NavigationStack {
            HomeView(vm: vm, onGoToSettings: { showingSettings = true })
        }
        .sheet(isPresented: $showingSettings) {
            NavigationStack(path: $settingsPath) {
                SettingsView(vm: vm, path: $settingsPath)
            }
        }
        // Auto-present settings when the env hook seeded a deep-link path.
        .onAppear {
            if !settingsPath.isEmpty {
                showingSettings = true
            }
        }
        // Post-pairing "解锁更多" carousel (keyboard → share → paste). Full-screen
        // (not a sheet) so the 教学页 hero bleeds to the status bar Paste-style —
        // a `.large` sheet keeps a system gap + grabber at the top and can't.
        // Dismissed via the floating ✕ / 完成 / 稍后. Raised once by
        // `presentEnhancementsIfDue` after first-run pairing, or by the
        // `UC_ONBOARDING_ENHANCE` hook below for screenshots.
        .fullScreenCover(isPresented: $showEnhancements) {
            OnboardingView(mode: .enhancements) { showEnhancements = false }
        }
        .task {
            // Unblock pasteboard reads before the engine ticks — the
            // engine's push path reads UIPasteboard via snapshot(), and
            // gating activation here is what defers the iOS "Allow Paste"
            // prompt past the Setup flow into the home tab.
            vm.activatePasteboard()
            vm.engine.start()
            presentEnhancementsIfDue()
            let env = ProcessInfo.processInfo.environment
            if env["UC_ONBOARDING_ENHANCE"] == "1" {
                showEnhancements = true
            }
            if env["UC_AUTO_PUSH"] == "1" {
                // Auto-push is opt-in now (default off — the headline push
                // path is the Home PasteButton). The screenshot recipe wants
                // the engine to push the env-seeded pasteboard, so enable the
                // opt-in for this run before kicking the tick.
                vm.appSettings.autoPushDeviceChanges = true
            }
            if env["UC_AUTO_PUSH"] == "1" || env["UC_AUTO_APPLY"] == "1" {
                vm.engine.forceTickNow()
            }
            if env["UC_AUTO_SAVE"] == "1" {
                await vm.saveServerAttachment()
            }
            // §2.11 regression hook: after the first history-sync wave
            // populates `vm.history`, save the first image/file entry
            // via the history endpoint. Sleep is intentional — simctl
            // recipes have no way to observe `vm.history` mutations,
            // so a small grace window beats polling here.
            if env["UC_AUTO_SAVE_HISTORY"] == "1" {
                try? await Task.sleep(nanoseconds: 3_000_000_000)
                if let target = vm.history.first(where: {
                    $0.entry.hasData && ($0.entry.type == .image || $0.entry.type == .file)
                }) {
                    await vm.saveAttachment(for: target)
                }
            }
            // §2.11 + UIPasteboard regression hook: same grace window
            // as save, but targets an image-type history row and writes
            // back to the device pasteboard. file-type rows aren't valid
            // — `applyAttachment` rejects them.
            if env["UC_AUTO_APPLY_HISTORY"] == "1" {
                try? await Task.sleep(nanoseconds: 3_000_000_000)
                if let target = vm.history.first(where: {
                    $0.entry.hasData && $0.entry.type == .image
                }) {
                    await vm.applyAttachment(for: target)
                }
            }
        }
        .onChange(of: scenePhase) { _, newPhase in
            switch newPhase {
            case .active:
                // Pick up history rows the keyboard / share extension appended
                // to the App Group while we were suspended, before the engine
                // starts mutating (and persisting) the in-memory log.
                vm.reconcileSharedHistory()
                // Refresh SSID on foreground so a Wi-Fi flip that happened
                // while backgrounded re-evaluates the §5.3 effective server
                // (`effectiveActiveConfig`) right away instead of waiting for
                // the next NWPathMonitor callback.
                vm.ssidProvider.refresh()
                // …and re-probe the candidate URLs (§5.3 Layer 2): the URL
                // that was live before backgrounding may be unreachable on
                // the network we woke up on. If the SSID refresh above also
                // fires onNetworkChanged, the in-flight dedup collapses the
                // two kicks into one probe.
                vm.kickLiveEndpointRefresh(force: true)
                vm.engine.isSceneInactive = false
                vm.engine.start()
            case .background:
                vm.engine.stop()
            case .inactive:
                // .inactive covers Control Center pull-down, incoming
                // calls, app switcher previews, etc. The system
                // pasteboard is still reachable, so we keep the loop
                // running — but at a reduced cadence (5s) so a
                // long-running .inactive (an answered call) doesn't
                // burn battery on per-second polling.
                vm.engine.isSceneInactive = true
            @unknown default: break
            }
        }
    }
}

#Preview {
    ContentView(vm: .preview())
}
