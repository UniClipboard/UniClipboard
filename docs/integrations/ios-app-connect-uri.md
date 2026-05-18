# iOS App integration: `uniclipboard://connect` deep link

**Audience**: developers of the native UniClipboard iOS App (repo `app.uniclipboard.UniClipboard`).
**Source of truth for the wire protocol**: `docs/architecture/mobile-sync-connect-uri.md` in
the desktop repo. Read it once before this guide — this document tells you *how* to wire
the protocol into the iOS App, not what the protocol means.

---

## 1. Why this guide exists

The desktop app's "Add mobile device" flow shows a QR code whose content is

```
uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url-json>
```

We want the user to point the **system Camera app** at that QR code and have iOS surface
a "Open in UniClipboard?" smart action that hands the URL to the App. The App then
parses the payload and either pre-fills the Add Server form or saves the server directly.

The iOS App already has an in-app `QRScannerView` that decodes a different QR format
(`ServerQRPayload`: plain JSON object, or `https://user:pass@host/`). Those formats are
**not URL-scheme URIs**, so the system Camera shows them as plain text and cannot route
them to the App. The system-camera flow only works when the QR content is a URL whose
scheme is registered by the App — that is the entire point of moving to
`uniclipboard://connect`.

Both paths can coexist:

| Path                                                          | Trigger              | Reaches the App via |
| ------------------------------------------------------------- | -------------------- | ------------------- |
| **A. System Camera → URL scheme** (primary, this document)    | iOS smart action     | `.onOpenURL`        |
| **B. In-app `QRScannerView`** (still supported)               | User taps "Scan"     | `onScan` callback   |
| **C. SyncClipboard Shortcut template** (legacy, see sibling)  | iOS Shortcuts        | Shortcut writes the three fields itself |

This guide covers A and the parser shared with B. C is documented separately in
`docs/integrations/ios-shortcut.md`.

---

## 2. Register the URL scheme

The App must declare `uniclipboard` as a URL scheme so iOS routes
`uniclipboard://…` URLs to it.

In Xcode: **Target `UniClipboard` → Info tab → URL Types → +**

| Field            | Value                                |
| ---------------- | ------------------------------------ |
| Identifier       | `app.uniclipboard.UniClipboard`      |
| URL Schemes      | `uniclipboard`                       |
| Role             | Editor                               |
| Icon             | (leave blank)                        |

This writes a `CFBundleURLTypes` entry into the generated `Info.plist`. Verify after
building:

```bash
plutil -p "$(xcodebuild -scheme UniClipboard -sdk iphonesimulator -showBuildSettings \
  2>/dev/null | awk -F'= ' '/BUILT_PRODUCTS_DIR/{print $2; exit}')/UniClipboard.app/Info.plist" \
  | grep -A 6 CFBundleURLTypes
```

> **Note**: `CFBundleURLTypes` is `array<dict>` and cannot be expressed via
> `INFOPLIST_KEY_*` build settings. The UI route above is the only stable way to
> declare it under the Xcode 26 generated-Info.plist model.

Do **not** register a Universal Link (Associated Domains). The desktop emits a
`uniclipboard://` URI, not an `https://` URL — there is no host you control to serve an
`apple-app-site-association` file for.

---

## 3. Wire `.onOpenURL` to the App

SwiftUI App lifecycle delivers incoming URLs via `.onOpenURL` on any view in the scene.
The cleanest place is the root scene in `UniClipboardApp.swift`, so the handler runs
regardless of which tab the user is on or whether `SetupFlowView` is active:

```swift
@main
struct UniClipboardApp: App {
    @State private var vm = AppViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView(vm: vm)
                .onOpenURL { url in
                    vm.handleIncomingURL(url)
                }
        }
    }
}
```

Add the dispatcher on `AppViewModel`:

```swift
extension AppViewModel {
    /// Entry point for `uniclipboard://…` URLs from the system Camera,
    /// Shortcuts, or any other UIApplication-level URL source.
    ///
    /// Routes:
    /// - `uniclipboard://connect?…` → `presentConnectURIPrefill(payload)`
    /// - everything else → ignore (forward-compat for future schemes)
    func handleIncomingURL(_ url: URL) {
        guard url.scheme?.lowercased() == "uniclipboard" else { return }
        guard url.host == "connect" else { return }

        do {
            let payload = try ConnectURI.parse(url.absoluteString)
            presentConnectURIPrefill(payload)
        } catch let error as ConnectURI.ParseError {
            presentConnectURIError(error)
        } catch {
            presentConnectURIError(.payloadDecodeFailed(detail: "\(error)"))
        }
    }
}
```

The two `present*` methods are UI surface decisions — see §6 for the recommended UX.

---

## 4. Parser implementation

Add a new file `Shared/Network/ConnectURI.swift` so it builds into both the App target
and the SwiftPM `UniClipboardNetwork` library (and is unit-testable via `swift test`).
Use pure Foundation only — no UIKit / SwiftUI / CryptoKit — per the `Shared/` rule in
the project `CLAUDE.md`.

### 4.1 Types

```swift
import Foundation

public enum ConnectURI {
    /// Parsed `uniclipboard://connect?…` payload, v1.
    public struct Payload: Equatable, Sendable {
        public let url: String
        public let user: String
        public let pwd: String
        /// Free-form metadata from the `o` object. Unknown keys are
        /// tolerated (spec §3.2). Known keys: `did`, `label`, `proto`,
        /// `install`. New keys may appear without a `v` bump.
        public let other: [String: String]

        public var label: String?    { other["label"] }
        public var deviceId: String? { other["did"] }
        public var proto: String?    { other["proto"] }
    }

    /// 1:1 with spec §4.2 error codes. Use `description` for log lines;
    /// for user-facing copy, switch on the case (see §7).
    public enum ParseError: Error, Equatable {
        case invalidScheme
        case unsupportedVersion(found: Int)
        case unsupportedService(found: String)
        case payloadDecodeFailed(detail: String)
        case missingField(name: String)
        case invalidURL(detail: String)
    }
}
```

### 4.2 The 6-step parser

These steps mirror the Rust and TypeScript implementations byte-for-byte. Any deviation
will cause the cross-language golden vector (§5) to fail.

```swift
public extension ConnectURI {
    static func parse(_ raw: String) throws -> Payload {
        guard let components = URLComponents(string: raw),
              components.scheme?.lowercased() == "uniclipboard"
        else { throw ParseError.invalidScheme }

        guard components.host == "connect" else { throw ParseError.invalidScheme }

        let queryItems = components.queryItems ?? []
        func q(_ name: String) -> String? {
            queryItems.first(where: { $0.name == name })?.value
        }

        // §4 step 2: version check
        guard let vStr = q("v"), let v = Int(vStr) else {
            throw ParseError.unsupportedVersion(found: 0)
        }
        guard v == 1 else { throw ParseError.unsupportedVersion(found: v) }

        // §4 step 3: service guard
        guard let svc = q("svc") else {
            throw ParseError.unsupportedService(found: "")
        }
        guard svc == "mobile-sync" else {
            throw ParseError.unsupportedService(found: svc)
        }

        // §4 step 4: base64url-no-pad → bytes
        guard let pParam = q("p") else {
            throw ParseError.payloadDecodeFailed(detail: "missing p")
        }
        guard let jsonBytes = base64URLDecode(pParam) else {
            throw ParseError.payloadDecodeFailed(detail: "invalid base64url")
        }

        // §4 step 5: bytes → JSON dict
        let raw: Any
        do {
            raw = try JSONSerialization.jsonObject(with: jsonBytes, options: [])
        } catch {
            throw ParseError.payloadDecodeFailed(detail: "\(error)")
        }
        guard let dict = raw as? [String: Any] else {
            throw ParseError.payloadDecodeFailed(detail: "payload is not a JSON object")
        }

        // §4 step 6: required field extraction. Empty == missing
        // (spec §4.2 collapses null / missing / empty into MISSING_FIELD).
        func requiredString(_ key: String) throws -> String {
            let s = (dict[key] as? String) ?? ""
            guard !s.isEmpty else { throw ParseError.missingField(name: key) }
            return s
        }

        // Embedded v must match the query v — extra defense against
        // hand-edited URIs.
        if let embeddedV = dict["v"] as? Int, embeddedV != 1 {
            throw ParseError.unsupportedVersion(found: embeddedV)
        }

        let urlString = try requiredString("url")
        let user      = try requiredString("user")
        let pwd       = try requiredString("pwd")

        // §5 in the spec: URL must be http(s).
        guard let parsed = URL(string: urlString),
              let scheme = parsed.scheme?.lowercased(),
              scheme == "http" || scheme == "https"
        else { throw ParseError.invalidURL(detail: urlString) }

        // §3.2: forward-compatible — drop non-string `o.*` values silently.
        var other: [String: String] = [:]
        if let o = dict["o"] as? [String: Any] {
            for (k, v) in o {
                if let s = v as? String { other[k] = s }
            }
        }

        return Payload(url: urlString, user: user, pwd: pwd, other: other)
    }

    /// base64url-no-pad → Data. Matches Rust `URL_SAFE_NO_PAD` and the
    /// TypeScript `btoa(...).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'')`.
    private static func base64URLDecode(_ s: String) -> Data? {
        var t = s.replacingOccurrences(of: "-", with: "+")
                 .replacingOccurrences(of: "_", with: "/")
        // Restore padding so Foundation accepts it.
        let pad = (4 - t.count % 4) % 4
        t += String(repeating: "=", count: pad)
        return Data(base64Encoded: t)
    }
}
```

> **Note**: we deliberately do **not** implement an encoder. The desktop is the only
> party that mints connect URIs; an iOS-side encoder would be dead code and an additional
> surface that could drift from the spec.

---

## 5. Cross-language golden test

This is the most important test in the iOS App for this feature. The desktop's Rust and
TypeScript tests already assert against the same string — if all three agree, the
protocol is byte-stable in practice.

Create `Tests/UniClipboardNetworkTests/ConnectURITests.swift`:

```swift
import Foundation
import Testing
@testable import UniClipboardNetwork

/// Golden vector from `docs/architecture/mobile-sync-connect-uri.md` §7.1.
/// MUST equal the string in `connect_uri.rs:GOLDEN_URI` and
/// `mobileSyncConnectUri.test.ts`. If any of the three sides drift, this
/// test (or its peers) breaks and the diff points at the offender.
private let goldenURI =
    "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ"

@Test
func parsesTheGoldenVector() throws {
    let p = try ConnectURI.parse(goldenURI)
    #expect(p.url == "http://192.168.1.5:42720")
    #expect(p.user == "mobile_aabbccdd")
    #expect(p.pwd == "AbCdEfGhIjKlMnOpQrSt")
    #expect(p.deviceId == "did_0123abcd")
    #expect(p.label == "Test")
    #expect(p.proto == "syncclipboard")
}

@Test(arguments: [
    ("https://example.com/connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ",
        ConnectURI.ParseError.invalidScheme),
    ("uniclipboard://connect?v=2&svc=mobile-sync&p=eyJ2IjoxfQ",
        .unsupportedVersion(found: 2)),
    ("uniclipboard://connect?v=1&svc=other&p=eyJ2IjoxfQ",
        .unsupportedService(found: "other")),
    ("uniclipboard://connect?v=1&svc=mobile-sync&p=not-valid-base64!@#",
        .payloadDecodeFailed(detail: "invalid base64url")),
])
func rejectsNegativeVectors(input: String, expected: ConnectURI.ParseError) {
    #expect(throws: expected) { try ConnectURI.parse(input) }
}
```

Run with `swift test --filter ConnectURITests`. Add the remaining §7.2 vectors
(`MISSING_FIELD`, `INVALID_URL`) the same way.

---

## 6. UX: how the App should react when a URI arrives

`AppViewModel.handleIncomingURL` is invoked from `.onOpenURL`, which can fire **at any
time** — during `SetupFlowView`, on the Home tab, while a modal is up. The dispatcher
must work from any state. Recommended decisions:

| Current state                                     | Recommended action                                                                                                                                                                                                                                                                                       |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vm.servers.configs.isEmpty` → `SetupFlowView`     | Push directly to `ServerFormStepView` with the three fields pre-filled. Do **not** auto-save — the user should see what they're committing to. The existing `UC_PREFILL=1` env hook already shows the path; productize that as a real `Step.formPrefilled(name, url, user, pwd)` case.                   |
| `vm.servers.configs.nonEmpty` → Home/Settings tab | Present a sheet ("Add new server from QR?") with the parsed `url` / `user` / a masked `pwd`. On confirm, append a `ServerConfig`; on cancel, dismiss. **Do not** silently replace the active server — the user might have scanned a guest device QR.                                                       |
| Sheet/modal already up (e.g. RotateMobilePassword) | Queue the URL: store it on the view model and consume it after the current modal dismisses. SwiftUI delivers `.onOpenURL` even when sheets are presented, but stacking another sheet on top is jarring.                                                                                                  |

Errors (§7) should never auto-dismiss. The user came from the Camera app and may not
realise scanning even succeeded; an inline alert with a one-line explanation is
necessary.

---

## 7. Error → user-facing copy

Map `ConnectURI.ParseError` to localized strings in `Localizable.xcstrings`. Stay
truthful — vague "QR code error" copy makes people retry instead of getting help.

| `ParseError` case                | Suggested key                                | Suggested zh-Hans                                              | Suggested en                                                            |
| -------------------------------- | -------------------------------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `invalidScheme`                  | `connectURI.error.invalidScheme`             | 这个二维码不属于 UniClipboard。                                | This QR code isn't a UniClipboard sync link.                           |
| `unsupportedVersion(found:)`     | `connectURI.error.unsupportedVersion`        | 二维码版本是 v%lld，本 App 暂不支持，请更新 App。               | QR uses protocol v%lld, which this App version doesn't recognize yet.   |
| `unsupportedService(found:)`     | `connectURI.error.unsupportedService`        | 二维码声明的服务 "%@" 不是手机同步。                           | The QR's declared service "%@" is not mobile sync.                      |
| `payloadDecodeFailed(detail:)`   | `connectURI.error.payloadDecodeFailed`       | 二维码内容损坏，请在桌面端重新生成。                            | QR payload is corrupted. Re-generate it on the desktop.                 |
| `missingField(name:)`            | `connectURI.error.missingField`              | 二维码缺少必需字段 "%@",请在桌面端重新生成。                  | QR is missing required field "%@". Re-generate it on the desktop.       |
| `invalidURL(detail:)`            | `connectURI.error.invalidURL`                | 二维码里的服务地址不合法。                                     | QR contains an invalid server URL.                                      |

These strings are advisory — the App's brand-voice committee may rephrase them. The
mapping itself is normative.

---

## 8. Reusing the parser inside `QRScannerView`

Path B in §1 — the in-app scanner — should accept `uniclipboard://connect` URIs in
addition to the legacy `ServerQRPayload` formats. Extend the dispatch point in
`ServerQRPayload.parse` (or the call site of it), not the connect-URI parser:

```swift
public extension ServerQRPayload {
    static func parse(_ raw: String) -> ServerQRPayload? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)

        // New primary path: connect URI from the desktop.
        if trimmed.lowercased().hasPrefix("uniclipboard://") {
            if let p = try? ConnectURI.parse(trimmed) {
                return ServerQRPayload(
                    name: p.label, url: p.url, username: p.user, password: p.pwd
                )
            }
            // Fall through to nil — don't pretend a malformed connect
            // URI is a legacy JSON QR. The scanner's existing "unable to
            // recognize" alert is the right surface.
            return nil
        }

        // Legacy paths preserved as-is.
        // … existing JSON-object and URL-with-userinfo branches …
    }
}
```

The unified entry point keeps both surfaces (system Camera and in-app scanner) feeding
the same `ServerForm` with identical pre-fill semantics.

---

## 9. Manual testing without a desktop

Generate a fresh connect URI from the desktop or paste a known-good one (§7.1) and
inject it into the simulator:

```bash
# Boots a sim if needed and routes the URL to the installed App.
xcrun simctl openurl booted \
  'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ'
```

Combine with `UC_FRESH=1` to test the empty-state path through `SetupFlowView`, and
without it to test the "App already has servers" path through the confirmation sheet.

Negative-path checks: paste the §7.2 vectors into the same `simctl openurl` command;
each should surface its mapped error message.

---

## 10. Maintenance

- **Protocol changes** happen in the desktop repo's `docs/architecture/mobile-sync-connect-uri.md`.
  When the spec adds a new `o.*` key (§10 v2 sketch), no iOS code change is needed —
  the parser already preserves unknown string values in `Payload.other`.
- **Protocol `v` bump** does require iOS work: bump the accepted version in
  `parse(_:)`, update the golden vector, and decide migration UX (refuse, warn, or
  silently accept).
- **Test drift detector**: if `ConnectURITests.parsesTheGoldenVector` ever fails after
  a desktop release, the desktop encoder probably regressed — compare against
  `connect_uri.rs` and the desktop TS test (`mobileSyncConnectUri.test.ts`).
- **Localization keys** belong in the iOS App's `Localizable.xcstrings`; the mapping
  table in §7 above is the contract.
