# Keyboard Extension — Design Doc

Status: **Proposal**. Not yet implemented. This doc captures the goals,
non-goals, and the design decisions we have to make *before* writing any
Swift, so we can argue with the design instead of with code.

The product idea: ship a Custom Keyboard Extension that, when the user
switches to it from the system keyboard switcher (🌐), shows the latest text
entries synced from the configured SyncClipboard server. Tapping an entry
inserts it into whatever input field is currently focused. The "keyboard"
does not type — it is a clipboard picker that happens to be reachable from
every text field on the OS.

This complements the existing Share Extension, which is the *upload*
direction (current selection → server). The keyboard extension is the
*download* direction (server → currently focused text field) without the
ceremony of: switch to UniClipboard → copy → switch back → long-press →
paste.

---

## 1. Goals

1. From any text input on iOS, let the user paste a recently-synced text
   clipboard entry in **two taps**: 🌐 → entry.
2. Reuse `Shared/Models` + `Shared/Network` end-to-end — the extension is
   another consumer of `SyncClipboardClient`, not a parallel protocol
   implementation.
3. Read the same `ServerConfigList` and `trustInsecureCert` flag that the
   main app + Share Extension already write to the App Group. The keyboard
   does not have its own server picker UI for the v1; whichever server is
   active in the main app is the one it talks to.
4. Degrade gracefully on Full Access denied: show a single explanatory cell
   with a deep link back into the main app's onboarding for Full Access,
   never crash, never silently no-op.

## 2. Non-goals (v1)

- **Typing.** No QWERTY, no autocomplete, no shift/return/space keys beyond
  the system-mandated globe + delete affordances. The keyboard is a list.
  If the user wants to type, they switch back to the system keyboard.
- **File entries** (`type: "file"`, non-image). `UIPasteboard` has no
  generic "file" slot — writing a file row would degrade to writing its
  `dataName` as a string, which is just the filename, useless. File rows
  are filtered out of the list.
- **Editing / pinning / search.** The list is the last *N* text entries,
  newest first. v1 ships without search, without pinning, without delete.
  Add later if telemetry shows the list gets unwieldy.
- **Server switching from inside the keyboard.** The keyboard reads the
  active server; it does not let you change it. Changing servers is a
  main-app affordance.
- **Push / live updates while the keyboard is on-screen.** v1 fetches once
  on `viewDidAppear` and offers a manual refresh control. No background
  polling, no websocket. The keyboard's lifetime is seconds, not minutes.

## 3. Hard constraints from iOS

These are not design choices; they are platform rules that the design has
to bend around.

### 3.1 Full Access (`RequestsOpenAccess`)

Custom Keyboards run sandboxed by default. Without Full Access:

- **No network.** `URLSession` requests fail immediately. The keyboard
  cannot reach the SyncClipboard server.
- **No shared `UserDefaults`.** `UserDefaults(suiteName: appGroupID)`
  returns a defaults object that *appears* to work but reads/writes a
  separate, per-extension store. The active `ServerConfigList` written by
  the main app is invisible.

Both of those are non-negotiable for our use case, so **Full Access is
required**. When the user enables our keyboard in Settings → General →
Keyboards, iOS shows the standard warning that the keyboard developer can
access everything the user types. We have to address this head-on in the
onboarding flow (see §6).

### 3.2 What the keyboard *can* see in the host app

- `UITextDocumentProxy.documentContextBeforeInput` / `…AfterInput` — the
  text already in the input field, both sides of the caret. Useful for
  context-aware behavior (e.g., "this field looks like an email, prioritize
  email-shaped entries") but **not** required for v1.
- `UITextDocumentProxy.insertText(_:)` and `deleteBackward()`. That's the
  full programmatic write API. No image insertion, no rich text, no
  attributed strings — `UITextDocumentProxy` is text-only by design.

The keyboard cannot read the host app's selection, the host app's
`UIPasteboard`, or anything outside the focused text field. So the
"upload current selection" direction does not work from a keyboard
extension — Share Extension remains the only viable path for that.

### 3.3 Image entries — the `UIPasteboard` workaround

There is no direct image-insertion API for keyboards, but with Full Access
on, a keyboard *can* write to `UIPasteboard.general`. This is what every
GIF / sticker keyboard on the App Store does (Giphy, Tenor, the built-in
Memoji stickers). The tap flow:

1. User taps an image tile in our keyboard.
2. We write the image `Data` to `UIPasteboard.general` with a sensible
   UTI (`public.png` / `public.jpeg`).
3. We flash a one-shot toast over the tile: "已复制,在输入框长按 → 粘贴".
4. User long-presses in the host field and taps "粘贴".

Tradeoffs to accept before shipping this:

- **iOS 14+ paste banner.** "Pasted from UniClipboard" appears every
  time. There is no API to suppress it; it is the OS's anti-snooping UI
  for pasteboard reads. We do not control it.
- **Two extra taps.** Image insertion is 4 actions vs. 2 for text. We
  cannot close this gap without OS support.
- **Host compatibility is uneven.** Rich-text fields (Messages, Mail,
  Notes, WeChat) accept image paste. Plain-text fields (password,
  URL bar, single-line search) either refuse or coerce to a useless
  string. We do not try to detect this; if paste fails, that is the
  host app's behavior and the user will move on.

The two non-obvious parts — **how images get into the grid** and
**how the keyboard survives doing this in 48 MB** — are large enough
to deserve their own subsections.

#### 3.3.a Thumbnails without OOM

The SyncClipboard protocol has no thumbnail endpoint. To render an image
tile we have to fetch the full file from the server. The naïve
`UIImage(data:)` decode on a 4000×3000 JPEG materializes ~46 MB of
bitmap and kills the keyboard process. The non-negotiable rule:

> **Never construct a `UIImage` from full-resolution image data inside
> the keyboard process.** Always go through ImageIO's
> `CGImageSourceCreateThumbnailAtIndex` with
> `kCGImageSourceThumbnailMaxPixelSize` set to the tile's pixel width
> (~ 200–300 px @ 2x). ImageIO streams from disk and never inflates the
> source.

The flow per image entry:

1. Download the full image bytes from the server to App Group
   `Caches/keyboard/<server-id>/full/<hash>.<ext>` (disk, *not* memory).
   Cap total cache size to ~20 MB with LRU eviction.
2. Generate a thumbnail with ImageIO from that file, write the PNG to
   `Caches/keyboard/<server-id>/thumb/<hash>.png`. Thumbnails are small
   (a few KB each) — no eviction needed under reasonable cap of 50
   entries.
3. The waterfall row renders the thumbnail via `Image(uiImage:)`
   constructed from the small PNG. The full bytes stay on disk until
   the user taps the tile, at which point we re-read the full file and
   hand the `Data` directly to `UIPasteboard.general` (no `UIImage`
   round-trip — the pasteboard takes raw bytes).

#### 3.3.b Pre-fetch policy

A list view can lazy-fetch as the user scrolls; a waterfall needs
thumbnails up front to size tiles. Compromise:

- On keyboard appear, fetch the latest entry from
  `GET /SyncClipboard.json`. If it's an image new to the cache, kick off
  background download + thumbnail.
- The first time the user ever opens the keyboard against a given
  server, the image cache is empty — the grid will be mostly empty,
  with only text rows (and maybe one image tile). This is expected and
  not a bug. The cache grows as the user re-opens the keyboard over
  time and as the server's clipboard changes.
- Past-entry backfill is **not** in v1. The protocol exposes one
  current entry; we don't synthesize history by re-asking. If the main
  app ever exposes a history endpoint, the keyboard can hydrate from
  it on appear.
- Each appear, kick off thumbnail generation for any cached-but-not-yet-
  thumbnailed entries (e.g., interrupted previous session). Bound the
  work to one image at a time to stay under the memory ceiling.

The honest framing: image support is "copy + manual paste", not "tap to
insert". The tile's accessory is a 📋 glyph in the corner, not an
insert chevron, so users learn the gesture is different.

### 3.4 Memory budget

Keyboard extensions have a tight memory limit (around 48 MB historically;
Apple does not publish a guaranteed number). Image rows make this a real
constraint — see §3.3 for the rules (raw `Data` only, no `UIImage`
decode of full-res, no pre-fetch). Even for text rows, keep the list to
~50 entries and truncate row previews to a couple hundred characters.

### 3.5 No `UIApplication.shared`

App-extension SDK rejects `UIApplication.shared` and friends. Anything in
`Shared/` that the keyboard depends on must already be UIKit-clean (it is,
per the existing `Shared/` rule for the Share Extension). Opening the main
app from the keyboard for onboarding goes through
`extensionContext?.open(_:completionHandler:)` — supported on keyboard
extensions only if Full Access is granted, otherwise we have to instruct
the user to leave the keyboard manually. v1 chooses the latter (we cannot
require Full Access to *ask for* Full Access).

## 4. UX

Layout: **two-column staggered waterfall** (`Pinterest`-style). Text
tiles size to content (clamped to 4 lines + ellipsis). Image tiles size
to the thumbnail's aspect ratio at column width. The keyboard's
intrinsic height is ~260 pt by default, so only the top row is visible
without scrolling — `ScrollView { LazyVGrid }` handles the rest.

```
┌─────────────────────────────────────────┐
│ UniClipboard · stub-server          ↻   │  ← sticky header
├─────────────────────┬───────────────────┤
│ 你好,世界          │ ┌───────────────┐ │
│                ›   │ │ [thumbnail]   │ │
│           2m ago   │ │            📋 │ │  ← image tile: thumb + corner glyph
├─────────────────────┤ │      14m ago  │ │
│ https://example.   │ └───────────────┘ │
│ com/very/long-url… │ scripts/sync-     │
│                ›   │ stub-server.py    │
│           5m ago   │              ›    │
│                    │            18m ago│
├─────────────────────┴───────────────────┤
│ 🌐                              ⌫        │
└─────────────────────────────────────────┘
```

- Tap a **text tile** → `insertText(rowText)` → no dismiss, no toast.
- Tap an **image tile** → read full bytes from disk cache → write to
  `UIPasteboard.general` → tile flashes "已复制,长按粘贴" overlay for
  ~3s. No insertion happens automatically; the user pastes in the host
  field.

Two columns, not three: at typical keyboard widths (320–430 pt), three
columns make text unreadable and thumbnails too small to recognize.
SwiftUI's `LazyVGrid` with a fixed pair of `GridItem(.flexible())`
gets us the staggered behavior for free, because each tile's height is
intrinsic.
- Header row stays sticky. The refresh control re-runs the same fetch we
  did on appear.
- Globe is a long-press menu on iOS; we get it for free from
  `UIInputViewController.handleInputModeList(from:with:)`.
- Delete-backward is included because users will mis-tap, and the system
  keyboard is two taps away. This is the *only* concession to "real
  keyboard" behavior.

Empty / error states:

| State | Cell content |
|---|---|
| No server configured | "请在 UniClipboard 主程序中添加服务器" + tap to open main app |
| Full Access denied | "需要在「设置 → 通用 → 键盘」中开启「允许完全访问」" + a short rationale |
| Fetch failed | The error sentence from `SyncError.localizedDescription`, with a retry button |
| No text entries (only image/file on server) | "暂无文本剪贴板条目" |
| Loading | A single shimmer row, no spinner |

All copy goes through `Localizable.xcstrings`. The keyboard extension
target needs the catalog added to its membership.

## 5. Data flow

```
                       ┌──────────────────────────┐
                       │   SyncClipboard server   │
                       └────────────▲─────────────┘
                                    │  GET /SyncClipboard.json
                                    │
    ┌───────────────────────────────┴────────────────────────────────┐
    │                                                                │
    │  Keyboard extension (Full Access ON):                          │
    │    SyncClipboardClient.fetchLatest()                           │
    │                          │                                     │
    │                          ▼                                     │
    │       Filter type == "text", drop the entry whose hash         │
    │       matches lastSyncedContentHash in App Group (it's         │
    │       the one we just pushed from this device — don't          │
    │       offer it back).                                          │
    │                                                                │
    │    Append entry metadata into a local rolling cache (App Group │
    │    UserDefaults, keyed by `keyboard_history_v1`, max 50,       │
    │    FIFO, scoped by ServerConfig.id).                           │
    │                                                                │
    │    For image entries, additionally:                            │
    │      - download full bytes to                                  │
    │        Caches/keyboard/<server-id>/full/<hash>.<ext>           │
    │      - generate thumbnail to                                   │
    │        Caches/keyboard/<server-id>/thumb/<hash>.png            │
    │        via ImageIO (never UIImage(data:)).                     │
    │    Render the waterfall from the cache + thumbnail PNGs.       │
    │                                                                │
    └────────────────────────────────────────────────────────────────┘
```

Why a local cache instead of just rendering the single latest entry the
server returns? Because SyncClipboard's protocol exposes *one* current
clipboard, not a history. The cache is the keyboard's own running log of
"text entries I have seen on this server." It is keyed per `ServerConfig.id`
so switching active server in the main app doesn't pollute the list.

The cache is **append-on-fetch**, never written to by `UIPasteboard`
observation — the keyboard cannot observe the system pasteboard. The main
app's `SyncEngine` could, in principle, write to the same cache to keep it
fresh between keyboard appearances, but that is a v1.1 optimization. In v1
the cache only grows when the keyboard is on screen and a fetch lands a
new entry.

## 6. Onboarding (Full Access)

This is the riskiest part of the design and worth a section.

When a user first installs UniClipboard, the keyboard is **not enabled**.
They have to go to Settings → General → Keyboards → Keyboards → Add New
Keyboard → UniClipboard → tap row → toggle "Allow Full Access". iOS shows
its scary warning on that toggle. Most users will bounce.

Plan:

1. Add a settings row in the main app under a new section "扩展功能 →
   键盘" with three states:
   - **Not added yet** → CTA "在系统设置中添加 UniClipboard 键盘"
   - **Added but Full Access OFF** → CTA "开启完全访问以同步服务器内容"
   - **Ready** → ✅ + "在任意输入框点按 🌐 切换到 UniClipboard 即可"
2. Detect "added" by listening for `UIInputViewController` extension
   installation — actually iOS does not give the main app this signal
   directly. The cheap proxy is "I cannot detect it; instead, show a
   walkthrough screen with screenshots and let the user tap '我已添加'
   manually." Defer real detection to v1.1 if it matters.
3. Detect Full Access from the extension side via
   `UIInputViewController.hasFullAccess`. The keyboard renders the
   "denied" empty state described above and asks the user to flip the
   toggle. The main app cannot read this property; only the extension
   process can.
4. The Full Access rationale text needs to be honest and specific:
   > "完全访问让 UniClipboard 键盘能够联网读取你的服务器剪贴板内容,
   > 并使用与主程序共享的服务器配置。我们不会上传或记录你在其他应用
   > 中输入的任何文本。"
   The second sentence is a promise we need to back up by literally not
   calling any text-capture API. Code review checklist: the keyboard's
   `textDidChange(_:)` and `textWillChange(_:)` must remain empty (or
   only used for local UI state).

## 7. Code layout

Proposed (no code yet; this is a plan):

```
UniClipboardKeyboard/                ← new Xcode target (app extension,
│                                       NSExtensionPointIdentifier =
│                                       com.apple.keyboard-service)
├── Info.plist                       ← NSExtension manifest;
│                                       RequestsOpenAccess = YES;
│                                       PrimaryLanguage = zh-Hans
├── UniClipboardKeyboard.entitlements ← app group only (keyboards do not
│                                       get keychain-sharing or wifi-info)
├── KeyboardViewController.swift     ← UIInputViewController subclass;
│                                       hosts the SwiftUI root via
│                                       UIHostingController
├── KeyboardRootView.swift           ← SwiftUI: header + list + footer
├── KeyboardGridViewModel.swift      ← @Observable; owns the fetch +
│                                       cache + insertText / copy-image
│                                       callbacks
├── KeyboardHistoryStore.swift       ← App Group UserDefaults reader/
│                                       writer for keyboard_history_v1
│                                       (entry metadata only)
└── KeyboardImageCache.swift         ← App Group Caches/ reader/writer
                                        for full + thumb files; ImageIO
                                        thumbnail generation; LRU eviction
                                        at ~20 MB cap
```

Reused from `Shared/`:

- `Shared/Models/Clipboard.swift` for decoding the server response
- `Shared/Models/ServerConfig.swift` + `SettingsStore` for the active
  server lookup
- `Shared/Network/SyncClipboardClient.swift` for `fetchLatest()`
- `Shared/Network/SyncError.swift` for the error→string mapping

Nothing new lands in `Shared/`. If something feels like it wants to,
double-check that it's not UIKit-tainted before moving.

The Xcode setup mirrors `UniClipboardShare`: `PBXFileSystemSynchronizedRootGroup`
on `UniClipboardKeyboard/`, with `Info.plist` and the entitlements file
listed under `membershipExceptions` so they don't get double-processed
(the same "Multiple commands produce" footgun the Share target stepped on).

## 8. Test plan

- **Unit (SwiftPM):** add `KeyboardHistoryStoreTests` covering the rolling
  cache (FIFO at 50, per-server-id isolation, no-op on hash dedup). Add
  `KeyboardImageCacheTests` covering ImageIO thumbnail generation against
  a fixture JPEG, LRU eviction at the 20 MB cap, and that the cache
  never inflates a `UIImage` from full-res data (assert via instruments-
  style peak-memory check, or at minimum via a `#if canImport(UIKit)`-
  gated unit that loads a 20 MB fixture and confirms RSS stays bounded).
  Lives next to the existing `UniClipboardModelsTests`.
- **Manual on device:** the simctl + Custom Keyboard combination is
  notoriously flaky — keyboards added in a simulator do not always appear
  in the keyboard switcher of every host app. Treat the simulator as best
  effort and require one device pass before shipping.
- **Manual checklist:**
  1. Fresh install → keyboard not in switcher.
  2. Add keyboard in Settings (no Full Access) → switch to it in Notes →
     see the "需要完全访问" empty state.
  3. Toggle Full Access on → relaunch Notes → switch to UniClipboard →
     see header with active server name and list of text entries.
  4. Tap an entry → text appears in Notes.
  5. Change active server in main app → switch to keyboard again → list
     reflects the new server's history (its own cache slice).
  6. Disable network → tap refresh → see the `SyncError` message, not a
     crash or an empty list.
  7. With the stub server running 401 mode (`STUB_MODE=401`) → keyboard
     shows the auth-failed error, not a generic "fetch failed".

## 9. Open questions

1. **Do we want a way for the keyboard to *write* the inserted entry back
   as the active server clipboard?** I.e., when the user picks entry X
   and inserts it, should we also `PUT` it so other devices see "this is
   the current selection" again? Argument for: keeps cross-device state
   coherent. Argument against: the user already had this text on the
   server; re-PUTting it is noise. **Default: no.** Revisit if a tester
   asks.
2. **Should the keyboard read `documentContextBeforeInput` for context?**
   E.g., prioritize URL-shaped entries when the focused field looks like
   a URL bar. Cheap to add later; we don't need it for v1 and it muddies
   the "we don't look at what you type" promise unless we are precise
   about scope.
3. **Does iOS 17/18+ change anything for keyboard extensions?** The
   Live Activities / Interactive Widgets push has not touched keyboards
   meaningfully, but worth a 30-min spike before committing. (Our
   deployment target is iOS 17.0 per the recent commit `417be2c`.)
4. **Privacy manifest entries.** The keyboard target will need its own
   `PrivacyInfo.xcprivacy` if the main app already has one. Network
   access is the only declared reason.

## 10. Decision needed before coding

- [ ] Accept the "no file rows, image via copy + manual paste" non-goal.
- [ ] Accept that v1 requires the user to manually mark "I added the
      keyboard" in onboarding (no automatic detection).
- [ ] Confirm the Full Access copy in §6 is the wording we want, or
      replace it now.
- [ ] Confirm the cache key (`keyboard_history_v1`) and the 50-entry
      cap (text + image entries share the same cap).
- [ ] Confirm the image thumbnail disk cache cap (~20 MB) and its
      location (App Group `Caches/keyboard/<server-id>/`).
- [ ] Confirm two-column waterfall vs. dynamic column count (e.g.,
      three columns on iPad / landscape). v1 proposes fixed two.
- [ ] Confirm the image tile's accessory affordance — corner 📋 glyph
      + flash-overlay on tap, or something more explicit.

Once those four are answered, the implementation is a 1–2 day target:
new Xcode target, ~300 lines of Swift, reusing every model + network
type we already have.
