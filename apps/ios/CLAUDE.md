# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Product

UniClipboard is a SwiftUI iOS client for self-hosted clipboard sync. It is **wire-compatible with [SyncClipboard](https://github.com/Jeric-X/SyncClipboard)** — any SyncClipboard-protocol server works as a backend. UI copy treats `UniClipboard` as the primary brand and demotes `SyncClipboard` to a compatibility footnote (e.g. Settings → About reads "兼容 SyncClipboard v1"). Keep this distinction when writing new strings.

The wire protocol and persistence formats are normative: **`docs/SYNC_PROTOCOL.md` is the source of truth.** When code disagrees with the spec, update the spec to match reality (it says so itself). Round-trip JSON fixtures live under `docs/examples/` and are referenced by name in tests.

## Dual-build layout

The repository builds three ways and shares source between them:

- **Xcode app target `UniClipboard`** (`UniClipboard.xcodeproj`) compiles everything under `UniClipboard/` and `Shared/` via `PBXFileSystemSynchronizedRootGroup`. Drop a new `.swift`/`.xcstrings` file under either root and Xcode picks it up — no pbxproj edit needed.
- **Xcode share-extension target `UniClipboardShare`** compiles `Shared/` + `UniClipboardShare/`. The extension links nothing from `UniClipboard/` — anything it needs lives in `Shared/`. Two files inside `UniClipboardShare/` are excluded from the synchronized group via `PBXFileSystemSynchronizedBuildFileExceptionSet` (`Info.plist`, `UniClipboardShare.entitlements`) — without that, Xcode tries to both `ProcessInfoPlistFile` AND copy them as resources, hitting "Multiple commands produce" on the appex's `Info.plist`.
- **SwiftPM library + tests** (`Package.swift` at the repo root) re-uses `Shared/Models/` and `Shared/Network/` as target source paths so model + network layers can be unit-tested via `swift test` without provisioning in-Xcode test targets. Tests live in `Tests/UniClipboardModels{Network,}Tests/` and load fixtures from `docs/examples/` via `#filePath` — no resource copy or symlink.

The rule for `Shared/`: **pure Foundation, no UIKit / SwiftUI / CryptoKit-via-UIKit-only APIs**. The Share Extension's app-extension SDK rejects parts of UIKit (anything marked `API_UNAVAILABLE(macos)` is fine, anything `NS_EXTENSION_UNAVAILABLE` is not), and a SwiftPM `swift test` runs on macOS without UIKit at all. If something in `Shared/` ever needs platform code, gate it with `#if canImport(UIKit)` and keep callers happy on both sides.

Don't try to add SwiftPM `.copy("Fixtures")` with a directory symlink — SwiftPM doesn't follow it. The `#filePath` approach was chosen specifically because it survives without any sync step.

The Xcode target carries one Run Script build phase, **"Inject UIFileSharingEnabled (Xcode 26 INFOPLIST_KEY workaround)"**, that PlistBuddy-injects `UIFileSharingEnabled=true` into `${TARGET_BUILD_DIR}/${INFOPLIST_PATH}` after `ProcessInfoPlistFile`. Reason: Xcode 26's processInfoPlistFile silently drops `INFOPLIST_KEY_UIFileSharingEnabled` (sibling keys like `INFOPLIST_KEY_LSSupportsOpeningDocumentsInPlace` are honored), and we ship "save to Documents" — Files-app needs both keys to surface the folder under "On My iPhone". Two non-obvious bits keep this working: the script declares `$(TARGET_BUILD_DIR)/$(INFOPLIST_PATH)` as `inputPaths` (not `outputPaths`) so it runs *after* `ProcessInfoPlistFile` instead of before, and the target sets `ENABLE_USER_SCRIPT_SANDBOXING = NO` because the sandbox routes in-place writes through a copy-on-write overlay that's discarded on script exit (PlistBuddy returns 0 but the change vanishes). The script verifies the value via `Print` and fails the build if injection didn't persist — so if Apple ever fixes the underlying bug we'll notice and can revert.

## Commands

Run from the repo root.

```bash
# Run model round-trip tests (23 cases, ~50ms)
swift test

# Build the iOS app for the simulator
xcodebuild -scheme UniClipboard -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -configuration Debug build

# Run a single test by name
swift test --filter FixturesTests/test_clipboardNoHash_optionalKeysAreOmittedNotNullified
```

Bundle ids: `app.uniclipboard.UniClipboard` (app) + `app.uniclipboard.UniClipboard.Share` (extension). Deployment target iOS 26.2. Swift 5 mode; `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor` is on for both Xcode targets (so newly-written types under `UniClipboard/`, `UniClipboardShare/`, and `Shared/` are MainActor-isolated when compiled into either Xcode target). The SwiftPM `UniClipboardModels` / `UniClipboardNetwork` targets compile the same `Shared/` files unisolated — anything in `Shared/` that needs to run off-main must say so explicitly (`nonisolated`, an actor, or a non-MainActor class).

## Screenshots / debug runs

Useful for evaluating UI without manual taps. After installing the built `.app`:

```bash
APP="$(xcodebuild -scheme UniClipboard -sdk iphonesimulator -showBuildSettings 2>/dev/null \
  | awk -F'= ' '/BUILT_PRODUCTS_DIR/{print $2; exit}')/UniClipboard.app"
xcrun simctl install "iPhone 17 Pro" "$APP"
xcrun simctl launch "iPhone 17 Pro" app.uniclipboard.UniClipboard \
  -AppleLanguages '(zh-Hans)' -AppleLocale zh_CN
xcrun simctl io "iPhone 17 Pro" screenshot /tmp/shot.png
```

Pass per-launch env via `SIMCTL_CHILD_<NAME>=value` exported in the shell calling `simctl launch` — e.g., `SIMCTL_CHILD_UC_AUTO_PUSH=1 xcrun simctl launch booted app.uniclipboard.UniClipboard`. There is **no** `--setenv` flag on `simctl launch`; the `SIMCTL_CHILD_` prefix is the only documented way to inject env into the launched app. Locale via `-AppleLanguages '(en)'` etc. (the app supports `en` and `zh-Hans`; `zh-Hans` is the source language of the catalog).

### Launch-time env hooks (DEBUG-style)

| Env | Effect |
|---|---|
| `UC_INIT_TAB=0\|1` | Start on Clipboard / Settings tab |
| `UC_FRESH=1` | Boot with empty `ServerConfigList` → first-run onboarding, then the 未配置 Home |
| `UC_SETUP_STEP=form` | Bootstrap `SetupFlowView`'s path to the server form. **Currently inert** — ContentView no longer mounts SetupFlow; first-run pairing goes through Home/Settings `AddServerSheet` (`UC_OPEN_ADD_SHEET=1`) |
| `UC_PREFILL=1` | Prefill ServerForm fields with mock defaults |
| `UC_PREFILL_TEST=success\|authFailed\|unreachable\|missingFields` | Seed ServerForm test-connection result on appear |
| `UC_OPEN_SWITCHER=1` | Auto-present the ServerSwitcher sheet on Home (simctl has no synthetic-tap, so this is how the sheet gets screenshotted) |
| `UC_AUTO_SAVE_HISTORY=1` | After 3s grace window, save the first `image`/`file` entry in `vm.history` via §2.11. Companion to `UC_AUTO_SAVE`, which only covers the live-latest §2.4 path. |
| `UC_AUTO_APPLY_HISTORY=1` | After 3s grace window, apply the first `image` entry in `vm.history` to `UIPasteboard.general` via §2.11. File-type rows can't be applied (no meaningful UTI). |

These hooks are only present so the design can be inspected without an interactive simulator (simctl has no synthetic-tap API). Not feature flags; remove on the day this becomes a real product.

### Local stub server

`scripts/sync-stub-server.py` is a tiny stdlib-only HTTP stub that speaks just enough of the protocol to drive the iOS read path. The 200-mode body is byte-identical to `docs/examples/clipboard_text_short.json`, so any drift between the spec and the stub will surface immediately in the model fixture tests.

```bash
scripts/sync-stub-server.py                  # 200 + Hello, SyncClipboard!
STUB_MODE=401 scripts/sync-stub-server.py    # any int → that HTTP status
STUB_PORT=9000 scripts/sync-stub-server.py
```

To point a configured iOS simulator at the stub from outside the app, the value must be (a) `Data`, not `String` — `SettingsStore` reads `Data`; and (b) written into the **App Group container's prefs**, not the app's sandboxed prefs or the simulator user-domain plist. Since the Share Extension landed, `SettingsStore()` defaults to `UserDefaults(suiteName: "group.app.uniclipboard.UniClipboard")` so both processes see the same data. `xcrun simctl spawn booted defaults write …` writes the wrong plist; the per-app sandboxed plist is no longer read at all (except by the one-shot `.standard → group` migration on first launch). cfprefsd caches the suite plist on first read, so the only reliable recipe is **uninstall → install → pre-seed group container → launch**:

```bash
APP="$(xcodebuild -scheme UniClipboard -sdk iphonesimulator -showBuildSettings 2>/dev/null \
  | awk -F'= ' '/BUILT_PRODUCTS_DIR/{print $2; exit}')/UniClipboard.app"

SCL='{"configs":[{"id":"stub","url":"http://127.0.0.1:8033/","username":"u","password":"p","autoSwitchWifiNames":[]}],"activeConfigId":"stub"}'

# 1. Build a plist with server_config_list as Data.
cat > /tmp/uc-prefs.xml <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>server_config_list</key>
  <data>$(printf '%s' "$SCL" | base64)</data>
</dict>
</plist>
EOF

# 2. Reset, then drop the plist into the App Group container (not the
# per-app sandbox). The group container only exists after install, and
# its UUID is per-device — query simctl for the path.
xcrun simctl uninstall booted app.uniclipboard.UniClipboard
xcrun simctl install   booted "$APP"
GROUP=$(xcrun simctl get_app_container booted app.uniclipboard.UniClipboard \
        group group.app.uniclipboard.UniClipboard)
mkdir -p "$GROUP/Library/Preferences"
plutil -convert binary1 \
  -o "$GROUP/Library/Preferences/group.app.uniclipboard.UniClipboard.plist" \
  /tmp/uc-prefs.xml

# 3. Launch with whatever env hooks you need (NOT --setenv).
SIMCTL_CHILD_UC_AUTO_APPLY=1 xcrun simctl launch booted app.uniclipboard.UniClipboard
```

The legacy `server_config` key (pre-multi-server) doesn't need an explicit delete — `uninstall` wiped the whole sandbox already, and the new plist we drop in only carries `server_config_list`, so §5.5 migration sees no source data and stays out of the way. The `.standard → group` migration in `SettingsStore.init` is similarly idempotent: it short-circuits the moment it sees any known key already in the suite, so re-running this recipe doesn't fight it.

iOS 14+ ATS lets `127.0.0.1` through without an Info.plist exception, so plain HTTP works against the stub from the simulator.

## Architecture

```
Shared/                     # Compiled into BOTH Xcode targets + the SwiftPM
│                           # packages. Pure Foundation, no UIKit/SwiftUI.
├── Models/                 # Clipboard, ServerConfig, AppSettings, SettingsStore,
│                           # ServerNameGenerator. Also the SwiftPM
│                           # UniClipboardModels target.
└── Network/                # SyncClipboardClient, SyncError, ConnectionTester.
                            # Also the SwiftPM UniClipboardNetwork target.

UniClipboard/               # Main app target — UIKit/SwiftUI surface.
├── Pasteboard/             # UIPasteboard observer + UTI snapshot.
├── Sync/                   # SyncEngine (1Hz foreground tick) + SSID provider.
├── Mock/                   # In-memory fake state (servers / clipboard / history).
├── Views/
│   ├── Setup/              # Legacy first-run flow (Welcome → ServerForm). Not
│   │                       # mounted anywhere — pairing now goes through Home's
│   │                       # AddServerSheet; kept compiling pending removal.
│   └── *.swift             # HomeView (grouped time-descending list), SettingsView, components.
├── Localizable.xcstrings   # zh-Hans source, en translation.
├── AppViewModel.swift      # @Observable @MainActor root view-model.
├── ContentView.swift       # Root: first-run OnboardingView, else Home (Settings is a sheet).
└── UniClipboard.entitlements  # App Group + wifi-info entitlement.

UniClipboardShare/          # Share Extension target — receives system-share
│                           # attachments and pushes them to the active server.
├── Info.plist              # NSExtension manifest (NSExtensionPointIdentifier
│                           # = com.apple.share-services). Excluded from the
│                           # synchronized group via membershipExceptions.
├── UniClipboardShare.entitlements  # Same App Group; excluded same way.
├── ShareViewController.swift  # Principal class (UIViewController hosting SwiftUI).
├── ShareRootView.swift     # SwiftUI sheet: server picker + content preview + send.
├── ShareItem.swift         # NSItemProvider extraction (URL > text > image > file).
└── ShareUploader.swift     # §3.5 file-first PUT; advances lastSyncedContentHash
                            # in the App Group so the main app's SyncEngine
                            # doesn't echo the just-pushed entry back to device.
```

`ContentView` is the routing root: it switches between `SetupFlowView` and the three-tab `TabView` based on whether `servers.configs.isEmpty`. `AppViewModel` owns the engine, observer, and SSID provider; views bind via `@Bindable`. Persistence is `SettingsStore` (App Group), keyed by `AppSettings.PersistenceKey` (see §5.4–§5.5 of the protocol spec).

Key model invariants (failures here will be caught by `FixturesTests`):

- `Clipboard.hash` / `dataName` / `size` use **omit-nil-on-encode** discipline. Re-encoding `clipboard_no_hash.json` must NOT introduce any `"…": null` keys — only `type`, `text`, `hasData` survive.
- `hash` is uppercase 64-char hex SHA-256. Whitespace-only strings are normalized to `nil` on decode (so the encoder omits the key, not write `""`).
- `ServerConfigList.activeConfig` falls back to `configs[0]` when `activeConfigId` doesn't resolve, and to `nil` when `configs` is empty (network code MUST refuse to make calls in the latter case).
- `LegacyServerConfig.migrated()` is the exact one-shot path for users coming from the pre-multi-server format. The new key (`server_config_list`) replaces the old key (`server_config`) — see §5.5.

## App Group

Both Xcode targets carry `application-groups = ["group.app.uniclipboard.UniClipboard"]` in their entitlements (`UniClipboard/UniClipboard.entitlements`, `UniClipboardShare/UniClipboardShare.entitlements`). The constant lives once in code as `SettingsStore.appGroupID` — keep all three in sync if it ever moves.

`SettingsStore.init(defaults: UserDefaults? = nil)` defaults to `UserDefaults(suiteName: appGroupID)` and falls back to `.standard` only when the entitlement isn't active (e.g., the SwiftPM `swift test` harness, where the constant resolves but the suite is just a regular plist on disk). Tests inject an ephemeral `UserDefaults(suiteName:)` of their own — never `.standard`.

**One-shot migration**: the first time the suite is opened on an upgraded install, `migrateFromStandardIfNeeded(into:)` copies `server_config_list`, `app_settings`, `last_synced_content_hash`, and the legacy `server_config` from `.standard` → suite and removes the originals. It's gated on "no known key already present in the suite", so re-installs after the migration are no-ops and a `defaults write` into the suite from a test recipe won't be overwritten by stale `.standard` data.

**Share Extension write coordination**: when the extension successfully uploads a new clipboard entry it writes the entry's hash to `last_synced_content_hash` *before* returning. The main app's `SyncEngine`, on next tick, sees `server.hash == lastSyncedContentHash` and treats the entry as already synced — so we don't ping-pong the just-shared content back into the device UIPasteboard (which would trigger iOS's "Allow Paste" prompt for no benefit).

## Share Extension

The `UniClipboardShare` target is a share-services extension. When the user picks UniClipboard from the system share sheet:

1. iOS instantiates `ShareViewController` (`NSExtensionPrincipalClass = $(PRODUCT_MODULE_NAME).ShareViewController`). We host a `UIHostingController(rootView: ShareRootView(...))` instead of subclassing `SLComposeServiceViewController` — the compose-style chrome doesn't fit the server-picker + preview UX.
2. `ShareRootView` loads servers + the `trustInsecureCert` flag from the App Group `SettingsStore`, then `ShareItemExtractor.extract(from:)` pulls one attachment (URL > text > image > file priority) using `NSItemProvider.loadItem(forTypeIdentifier:completionHandler:)`.
3. On `发送`, `ShareUploader.upload(_:to:trustInsecureCert:)` runs the §3.5 file-first sequence: `putFile(name:body:)` if `hasData`, then `putClipboard(_:)`. On success it writes `lastSyncedContentHash` and dismisses via `extensionContext.completeRequest`.

`NSExtensionActivationRule` in `Info.plist` filters to: text (any count), URL (max 1), image (max 1), file (max 1). iOS evaluates this against attachment UTIs before showing UniClipboard in the share sheet; bumping the limits or adding new categories is a plist-only change.

**Manual verification** (simctl has no synthetic-tap, so this is the only path): build + install + open Safari → share button → look for UniClipboard. For files, share from the Files app; for images, from Photos. Inside the sheet, confirm the server name + content preview + size are correct, then 发送; the main app's `SyncEngine` should reflect the new entry within ~1s of being foregrounded.

**Don't put UIKit-specific code in `Shared/`** — the extension links against the app-extension SDK, which rejects `NS_EXTENSION_UNAVAILABLE` APIs (e.g., `UIApplication.shared`). Keep extension-specific UI/IO under `UniClipboardShare/`.

## i18n

`UniClipboard/Localizable.xcstrings` has `sourceLanguage: "zh-Hans"`. All Swift literals like `Text("剪贴板")` are catalog keys; the `en` translation is provided alongside. Adding a new locale is a catalog-only change (no Swift edits).

When you encounter a SwiftUI initializer that takes a Swift `String` value (not `LocalizedStringKey`) — e.g., `LabeledContent(_, value: String)`, `Text(stringVar)` — wrap the literal with `String(localized: "…")` or restructure to use a `Text` literal in a closure. The codebase already does this (see `formatSize` in `HomeView.swift` and the `(未命名)` fallback in `SettingsView.swift`). Don't hardcode `Locale(identifier: …)` in formatters — let `RelativeDateTimeFormatter` and `ByteCountFormatter` follow the system locale.

If two distinct Chinese strings collide on a single English translation (e.g., the app uses 服务器 in two roles), split them in the Swift code with different keys before adding to the catalog. The current split: `服务器列表` → "Servers" (the list page), `服务器` → "Server" (the badge on the home card).

## Theme gotchas

**`.buttonStyle(.borderedProminent)` needs an explicit foreground.** AccentColor's dark-mode variant is ivory (#F4F2EE), so SwiftUI's default white label disappears against the tint. Every `.borderedProminent` button in this codebase sets `.foregroundStyle(Color(.systemBackground))` on its Label — that flips with appearance and lands on a contrasting tone in both modes. Grep `borderedProminent` before merging any new button; missing this is a recurring regression. Same rule applies to anything else painted on top of `Color.accentColor` (custom capsules, banners, etc.).

## Design language — Liquid Glass tokens

The app targets a Paste-app-inspired visual language built on iOS 26 Liquid Glass. Every interactive surface follows a small set of reusable tokens defined in `UniClipboard/Views/LiquidGlassModifiers.swift`. **Exception: the keyboard extension uses no Liquid Glass and no materials at all.** Its tray (`UIInputView.Style.keyboard`) is already a system blur — glass on iOS 26+ — and glass cannot sample other glass: `glassEffect`/material elements hosted inside the keyboard render a translucent backdrop band + hairline artifact (observed on iOS 26/27 as "the header occludes the card row"). All keyboard surfaces go through the flat `flatSurface(in:)` helpers in `KeyboardRootView.swift`; don't reintroduce glass or materials there. Keyboard vertical metrics live in the `KeyboardLayout` enum — the controller's height constraint is computed from it, never hand-summed.

### Shape tokens

| Token | Modifier | Shape | Usage |
|---|---|---|---|
| Glass Circle | `.liquidGlassCircle()` | `Circle` | Icon-only buttons (search, sync, ⋯, error badge) |
| Glass Capsule | `.liquidGlassCapsule()` | `Capsule` | Text buttons (server picker, "选择") |
| Glass Card | `.liquidGlassCard(cornerRadius:)` | `RoundedRectangle` | Content surfaces (clipboard cards, panels) |

Each modifier resolves to `glassEffect(.regular, in: shape)` on iOS 26+ and falls back to `ultraThinMaterial` + a `0.06`-opacity primary stroke on earlier versions. **Do not use raw `.ultraThinMaterial` or `Color(.tertiarySystemFill)` for interactive surfaces** — always go through the token so the glass gate stays in one place.

### Standard button height: 52 pt

All floating action buttons — top bar, bottom toolbar, anywhere a glass circle or capsule appears — use **52 pt** as the fixed frame height. This includes:

- Bottom toolbar: search circle, server-picker capsule, sync circle
- Top bar: "选择" capsule, "⋯" circle, error badge circle
- Any future glass buttons in overlays or sheets

When a capsule contains text, set `.frame(height: 52)` on the content *inside* the capsule (before `.liquidGlassCapsule()`), not on the outer view — this keeps the capsule hugging the 52 pt content height instead of stretching to fill a parent.

### Buttons inside sheets (non-glass context)

Liquid Glass looks flat on a solid sheet background because there is nothing behind it to blur. For modal sheets (`IssueDetailSheet`, server-add forms, etc.) use **filled capsules** instead:

```swift
Text("Action label")
    .font(.subheadline.weight(.semibold))
    .foregroundStyle(.white)          // always white — works on any tint
    .frame(height: 48)
    .padding(.horizontal, 32)
    .background(tintColor, in: Capsule(style: .continuous))
```

Secondary / dismiss actions are plain text links (`.foregroundStyle(.secondary)`, no background).

**Never use `Color(.systemBackground)` as foreground on a tinted capsule** — it resolves to black in dark mode, which is unreadable on orange/yellow tints. Use `.white` unconditionally.

### Home page layout

The home page uses a **custom top bar + custom bottom toolbar** — the system navigation bar is hidden (`.toolbar(.hidden, for: .navigationBar)`), and there is no `TabView`. Settings is accessed via the ⋯ menu → "设置" (presented as a sheet). The top and bottom bars are both `safeAreaInset` views, not system chrome.

## Mock data, briefly

`Mock.swift` is a single namespace `enum Mock` with `servers`, `serverLatest`, `deviceClipboard`, and `history`. The latter is `[ClipboardHistoryItem]` (provenance-tagged Clipboards with timestamps and direction). `ClipboardHistoryItem` is a UI-only type — it's not persisted by the protocol. Replace `Mock.*` references with real state when you wire the network layer; nothing else should need to change.
