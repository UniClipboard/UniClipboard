# 0035 Probe Receipt · iOS Keyboard Extension Live Engine & Tailcat Sync

- Decision: [0035 iOS Keyboard Extension live sync](../decisions/proposed/0035-ios-keyboard-live-sync.md)
- Date: 2026-11-27
- Physical device: `marks iPhone` — iPhone 16 Pro (`iPhone17,1`), iOS 27.3 (`24C5037i`), arm64e
- Toolchain: Go `1.27.3` (`darwin/arm64`), Xcode `27.0` (`27A5194q`)

## Objectives

Before updating [0031](../decisions/proposed/0031-mobile-engine-integration.md) and implementing live bidirectional sync inside `app/expo/targets/keyboard`, empirically verify:

1. Real `phys_footprint` (iOS Jetsam accounting metric) and `os_proc_available_memory()` inside a real `com.apple.keyboard-service` extension process running `UnicCore` + `builtin.tailcat`.
2. Dynamic shared framework (`UnicCoreRuntime.framework`) linkage with `-fapplication-extension` so main app and `.Keyboard` share one binary copy.
3. Main App background `Engine.Close()` duration and dual-process App Group `DataDir` (`unic.lock`) handoff without `0xdead10cc` or `CodeInUse`.
4. Cold-start Tailcat connection latency, pre-connect capture flushing, bidirectional text + image sync between physical iPhone keyboard and Mac peer, and iOS 27 `UIPasteboard` paste permission behavior.

## Probe 1 · Isolated Go Engine + Real Tailcat Memory & Latency Baseline (macOS arm64)

Two real `mobile.Engine` OS processes paired over real `builtin.tailcat` (DERP + WireGuard), testing 1 KB text and ~1 MB noisy PNG bidirectional transfer:

| Stage | Default GC (`phys_footprint`) | `GOGC=25, GOMEMLIMIT=18MB` (`phys_footprint`) | Latency |
|---|---:|---:|---:|
| Process baseline | 7.6 MB | 7.8 MB | — |
| `NewEngine` + `Start` (warm profile) | 9.4 MB | 10.3 MB | 3–4 ms |
| Real Tailcat peer connected | 15.5 MB | 14.7 MB | 3.7–5.8 s |
| Send + receive 1 KB text | 15.8 MB | 15.0 MB | send 32 ms / recv 12 ms |
| Send + receive ~1 MB PNG | 22.1 MB | 18.0 MB | send 63 ms / recv 41 ms |
| `Close()` teardown | 17.2 MB | 17.3 MB | 2 ms (goroutines 157 → 1) |

## Probe 2 · Shared Dynamic Framework Packaging & Linkage

Wrapped `app/expo/modules/unic-engine/ios/Frameworks/UnicCore.xcframework` (54 MB static archive) into `UnicCoreRuntime.framework` using `-fapplication-extension -dynamiclib -Wl,-force_load`:

- Compiled cleanly for both `arm64-apple-ios17.0` and `arm64-apple-ios17.0-simulator` with zero extension-forbidden API warnings and no UIKit linkage (`Foundation`, `Security`, `Network`, `CoreFoundation`, `libresolv`).
- Device binary sizes:
  - `App.app/Frameworks/UnicCoreRuntime.framework/UnicCoreRuntime`: **45 MB** (embedded once)
  - `App.app/KeyboardProbe`: **201 KB** (`@rpath/UnicCoreRuntime.framework/UnicCoreRuntime`)
  - `PlugIns/UniClipboardKeyboard.appex/UniClipboardKeyboard`: **221 KB** (`@rpath/UnicCoreRuntime.framework/UnicCoreRuntime`)

## Probe 3 · Physical iPhone 16 Pro (iOS 27.3) Keyboard Extension & Mac Live Sync

Installed signed probe (`app.uniclipboard.shell` + `app.uniclipboard.shell.Keyboard`) onto physical iPhone 16 Pro, paired with live Mac Tailcat peer, backgrounded host app, switched to `Unic Probe` keyboard in a third-party app with Full Access enabled, and executed 3 full open/sync/close cycles (untuned default Go GC):

| Stage (Physical iPhone 16 Pro `.appex`) | `phys_footprint` | Available (`os_proc_available_memory`) | Jetsam Ceiling | Timing / Notes |
|---|---:|---:|---:|---|
| Host App `host_engine_started` | 18.4 MB | 3357.6 MB | 3376 MB | 61 ms cold / 13–29 ms warm |
| Host App `host_bg_engine_closed` (3 runs) | 24.6–27.0 MB | ~3350 MB | 3376 MB | **7–13 ms** inside `beginBackgroundTask` |
| `kb_viewDidLoad` | 11.5 MB | 165.5 MB | **177.0 MB** | `fullAccess=true` |
| `kb_after_NewEngine` (cold / warm) | 13.9 MB / 20.5 MB | 163.1 MB | 177.0 MB | **22 ms / 4 ms** |
| `kb_after_Start` | 15.3 MB | 161.7 MB | 177.0 MB | **0–2 ms** |
| `kb_captured_appear` (130-char text) | 17.9 MB | 159.1 MB | 177.0 MB | Triggered iOS system paste prompt (~1.6 s user tap) |
| `kb_peer_connected` (Cycle 1 / Cycle 2) | 18.0 MB / 20.8 MB | 159.0 MB | 177.0 MB | **2,305 ms / 5,194 ms** |
| `kb_received_applied` (Mac text → `UIPasteboard`) | 18.9 MB | 158.1 MB | 177.0 MB | `changeCount` 89 → 90 (and 90 → 91 in Cycle 2) |
| `kb_received_applied` (Mac 1 MB PNG) | 19.9 MB | 157.1 MB | 177.0 MB | Received in keyboard process cleanly |
| `kb_sent_png` (123 KB PNG KB → Mac) | **23.6 MB (peak)** | **153.4 MB** | 177.0 MB | Delivered to Mac `clip_items` (`seq=38`) |
| `kb_after_Close` (`viewWillDisappear` / cycle) | 20.6–21.3 MB | 155.7–156.4 MB | 177.0 MB | **12–52 ms**; 0 memory warnings, 0 Jetsam kills |

Mac peer SQLite `clip_items` confirmed real arrival of:
- `seq=35`: 300-byte UTF-8 text auto-captured on keyboard appear and flushed `+186 ms` after `kb_peer_connected`.
- `seq=38`: 123,444-byte PNG sent directly from the iPhone Keyboard Extension.
- `seq=39`: 38-byte text re-captured on Cycle 2 (exposing the cross-reopen anti-loopback requirement below).

## Findings Incorporated into 0035

1. **Persistent cross-process anti-loopback required**: Reopening the keyboard recreated `KeyboardViewController`, losing in-memory `lastAppliedChangeCount` and re-capturing `cc=90`. Fixed in design via shared App Group `keyboard-bridge/pasteboard-state.json`.
2. **Pre-connect flush queue required**: Initial capture happens ~20 ms after appear while `builtin.tailcat` connects in 2.3–5.2 s; session-scoped flush via `ResendHistoryItem` on `EventDevices(connected)` delivered within 186 ms.
3. **Clipboard image `fileName` must be empty**: `mobile.Engine.ImportBytes` treats non-empty `fileName` as `kind="file"` rather than `kind="representation"`.
4. **iOS paste permission dialog confirmed**: Reading another app's pasteboard on appear triggered the iOS system paste prompt; settings guidance to enable "Allow" under system "Paste from Other Apps" is included in 0035.
5. **JS store `instance_id` guard**: Reopening the Engine when the Main App returns from background yields a new `instance_id`; `app/expo/src/engine/store.ts` must compare against live `get().snapshot?.instance_id` instead of a stale closure.
