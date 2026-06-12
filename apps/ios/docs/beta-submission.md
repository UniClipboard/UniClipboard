# TestFlight External Beta — Submission Kit

This document is the single source of truth for the copy and metadata we hand
to App Store Connect when submitting **UniClipboard** for external TestFlight
review. Each section maps 1:1 to a field on App Store Connect (ASC).

Keep it under version control: every Beta App Review submission is reviewed
against the *current* values in ASC, so changing a field here without
mirroring it there (or vice versa) will cause drift between docs and reality.

---

## 0. At-a-glance

| Field | Value |
|---|---|
| App name (App Store) | UniClipboard |
| Bundle display name (Home Screen) | UniClip |
| Bundle ID (main app) | `app.uniclipboard.UniClipboard` |
| Bundle ID (share extension) | `app.uniclipboard.UniClipboard.Share` |
| Primary category | Productivity |
| Secondary category | Utilities |
| Marketing version | 1.0 |
| Build number | 7 |
| Minimum iOS | 26.2 |
| Primary language | Simplified Chinese (zh-Hans) |
| Additional localizations | English (en) |
| Contact email | pendapazols42@outlook.com |
| Apple ID | _to fill on ASC_ |
| SKU | `uniclipboard-ios` |

---

## 1. Beta App Description

External testers see this in the TestFlight app, on the public TestFlight
landing page, and inside the invitation email. It is **not** the App Store
description — it is allowed to be more candid about the beta nature of the
build.

### 1.1 Simplified Chinese (primary)

```
UniClipboard 是一个自托管的跨设备剪贴板同步工具。

把你部署的剪贴板服务器(兼容 SyncClipboard 协议)填进来,UniClipboard 会让你的
iPhone、iPad、电脑共用同一个剪贴板:
• 文本、图片、文件 三种内容随时双向同步
• 系统共享菜单直接发送:在任意 App 长按或点分享 → UniClip,即可把内容推到服务器
• 多服务器配置 + 按 Wi-Fi 自动切换:家里、公司、外出可以分别绑定不同服务器
• 扫码配对:对端展示 uniclipboard:// 二维码,iPhone 相机一扫即可加入
• 桌面长按 App 图标的快捷操作:一键「上传剪贴板」/「拉取最新」
• 全部数据走你自己的服务器,UniClipboard 自身不收集任何剪贴板内容

⚠️ 这是一个 BYOS(自带服务器)应用 —— 使用前请先准备好一台 SyncClipboard
服务器(可参考 https://github.com/Jeric-X/SyncClipboard 自建)。

本次 Beta 版本希望验证:首次连接流程、跨平台同步稳定性、Wi-Fi 自动切换、
系统共享扩展的内容兼容性。欢迎在 TestFlight 内点「发送反馈」直接提交问题。
```

### 1.2 English

```
UniClipboard is a self-hosted clipboard that follows your devices across
iPhone, iPad and desktop.

Point it at any SyncClipboard-compatible server you run, and your clipboard
just works everywhere:
• Text, images and files sync both ways in near real-time
• Share Extension: hit Share → UniClip from any app to push content
• Multiple servers with optional Wi-Fi-based auto-switching (home / office /
  on-the-go bind to different backends)
• QR pairing: scan a uniclipboard:// code from the desktop client to join
• Home Screen quick actions: long-press the icon for "Push" / "Pull latest"
• Everything stays on your own server. UniClipboard collects nothing.

⚠️ BYOS — you need a SyncClipboard-protocol server before signing in. See
https://github.com/Jeric-X/SyncClipboard for the reference implementation.

This beta is focused on the first-run pairing flow, cross-platform sync
reliability, Wi-Fi auto-switching, and Share-Extension content fidelity.
Please use TestFlight's "Send Beta Feedback" to report anything that looks
wrong.
```

---

## 2. What to Test

Shown to external testers underneath the Beta App Description. Keep it action-
oriented — testers do this, not read about it.

### 2.1 Simplified Chinese

```
本次外部 Beta 的重点测试项:

1. 首次连接
   • 设置 → 服务器 → 添加,手动输入 URL/用户名/密码
   • 或在桌面客户端展示 uniclipboard:// 二维码,用 iPhone 相机或 App 内扫码
   验证连接成功后是否自动跳到剪贴板页

2. 双向同步
   • iPhone 复制一段文字 → 检查桌面端是否在几秒内收到
   • 桌面端复制一张图片 / 一个文件 → 检查 iPhone 是否拉取到、能否预览/导出

3. 系统共享扩展
   • 在 Safari、备忘录、Photos、Files 等 App 里点分享 → UniClip
   • 检查内容预览、文件大小是否正确,发送后是否同步到其他设备
   • 重点验证不会在主 App 弹出「允许粘贴」回声

4. 多服务器与 Wi-Fi 切换
   • 添加 2 台以上服务器,为不同 Wi-Fi 绑定不同服务器
   • 切换 Wi-Fi 后观察活动服务器是否自动跟着切换
   • 关闭 Wi-Fi 自动切换时,手动切换是否生效

5. Home Screen 快捷操作
   • 长按 App 图标 → 选择「上传剪贴板」/「拉取最新」,验证是否立即执行

6. 边界场景
   • HTTP-only 局域网服务器、自签名证书(打开「信任不安全证书」开关)
   • 较大文件(≥10 MB)、超长文本、复杂格式(emoji / 多语言)
   • 服务器返回 401 / 404 / 5xx 时的提示是否清晰

发现任何崩溃、UI 错位、文本截断、同步丢内容,请通过 TestFlight 反馈附截图。
```

### 2.2 English

```
Focus areas for this external beta:

1. First-run pairing
   • Settings → Servers → Add, either by typing URL/user/pass or by scanning
     a uniclipboard:// QR code from a desktop client.

2. Two-way sync
   • Copy text on iPhone — confirm desktop sees it within a few seconds.
   • Copy an image / file on desktop — confirm iPhone fetches and previews
     or exports it.

3. Share Extension
   • Use the system share sheet from Safari, Notes, Photos, Files, etc.
   • Verify content preview and size are correct, and the just-shared item
     does NOT bounce back as an "Allow paste" prompt in the main app.

4. Multiple servers & Wi-Fi switching
   • Add 2+ servers and bind each to a Wi-Fi SSID.
   • Switch Wi-Fi and confirm the active server follows.
   • With auto-switch off, confirm manual switching still works.

5. Home Screen quick actions
   • Long-press the icon and trigger "Push" / "Pull latest".

6. Edge cases
   • HTTP-only LAN server, self-signed certs (toggle "trust insecure cert").
   • Large files (>=10 MB), very long text, emoji / mixed scripts.
   • Server returning 401 / 404 / 5xx — is the error message clear?

Crashes, layout breakage, truncated copy, or missing sync — please report via
TestFlight feedback with a screenshot.
```

---

## 3. Beta App Review — Information for Reviewer

This is the **most important** section to get right. Apple's beta reviewer
needs to be able to actually exercise the app; this is a BYOS client, so we
must hand them a working backend.

### 3.1 Demo account (server credentials)

External Beta requires at least one credential set the reviewer can sign in
with. Fill in a public, throwaway server that we control before submission.

```
Test server URL : https://beta-stub.uniclipboard.app/
Username        : reviewer
Password        : <fill before submitting; rotate after review>
```

> Operational note: this server should be a fresh SyncClipboard reference
> deployment with no real user data. Rotate the password after each review
> cycle. If the server is unreachable when Apple tests, the build is rejected
> with the "couldn't sign in" boilerplate — verify it's up the morning you
> submit.

### 3.2 Sign-in walkthrough (paste verbatim into Review Notes)

```
UniClipboard is a self-hosted clipboard client. It REQUIRES a
SyncClipboard-protocol server (https://github.com/Jeric-X/SyncClipboard) —
there is no Apple-side account and no first-party backend.

To exercise the app:

1. Launch UniClipboard. The first-run flow opens automatically.
2. Tap "手动输入 / Enter manually" (or scan QR if preferred).
3. Fill in:
     URL      : https://beta-stub.uniclipboard.app/
     Username : reviewer
     Password : <see Demo Account field above>
4. Tap "测试连接 / Test connection". You should see a green check.
5. Tap "保存 / Save" → "完成 / Done".
6. The Clipboard tab now shows the most recent server-side entry.
7. Copy anything in another app (e.g. Safari URL bar). Within ~1 second the
   Clipboard tab will show the new entry as "Pushed from this device".
8. To test the share extension: open Safari, hit Share → UniClip, choose the
   server, hit 发送 / Send.

This is the entire feature surface; there are no IAPs, no subscriptions, no
account creation flow, and nothing gated behind a paywall.
```

### 3.3 Why we need the entitlements / permissions we declared

Paste this block too — it answers the questions Apple's reviewer almost
always asks about iOS apps that touch the LAN.

```
Permissions / entitlements requested, and why:

• NSLocalNetworkUsageDescription
  Many users self-host on a LAN box (Synology, home server). The app uses
  the standard URLSession against the user-configured URL — no Bonjour, no
  mDNS scanning. iOS still surfaces the LAN prompt for any RFC1918 hostname.
  Copy: "需要访问本地网络以连接你部署的剪贴板服务器".

• NSAppTransportSecurity → NSAllowsLocalNetworking = YES
  Same reason — allows plain HTTP to RFC1918 / .local hosts only. Public
  HTTP is still blocked by ATS. HTTPS to internet-facing servers is the
  default and recommended.

• NSCameraUsageDescription
  Used solely to scan a uniclipboard:// QR code rendered by a peer device
  during the Add-Server flow. Camera is not invoked anywhere else.
  Copy: "需要访问相机以扫描服务器配置二维码".

• NSLocationWhenInUseUsageDescription
  iOS 13+ gates CNCopyCurrentNetworkInfo (the only API that returns the
  current Wi-Fi SSID) behind Location-When-In-Use. We use the SSID only to
  pick which of the user's configured servers to activate. No location
  data is logged, persisted, or sent off-device. The prompt is shown only
  when the user enables "auto-switch by Wi-Fi" in Settings.
  Copy: "需要位置权限以读取当前 WiFi 名称,用于自动切换服务器".

• com.apple.developer.networking.wifi-info
  Companion entitlement to the SSID read above.

• com.apple.security.application-groups (group.app.uniclipboard.UniClipboard)
  Shared container between the main app and the Share Extension, so a
  share-sheet upload can set "last synced hash" and prevent the main app's
  sync engine from echoing the same content back to UIPasteboard.

• UIFileSharingEnabled / LSSupportsOpeningDocumentsInPlace
  The clipboard can carry a file. Exposing the app's Documents folder via
  the Files app lets users open / save attachments without a custom
  document picker.
```

### 3.4 Things the reviewer may notice but are intended

```
• On first launch the app shows a 3-screen onboarding flow (Welcome → Server
  → optional Wi-Fi binding). This is by design — there is no "explore as
  guest" mode because the app has nothing useful to show without a server.

• HTTP (not HTTPS) URLs are accepted in the server form. This is intentional
  for LAN-only deployments. ATS is left intact for public hosts; only
  RFC1918 / .local addresses are unblocked via NSAllowsLocalNetworking.

• The "信任不安全证书" toggle disables TLS validation for the configured
  server. It's clearly labelled "仅 LAN 使用 / LAN-only" and is off by
  default. It exists because self-hosted users often run with self-signed
  certs.

• The app does not request notifications, contacts, microphone, photos
  library, or background-execution entitlements. The clipboard sync engine
  is foreground-only (1 Hz tick while the app is active).
```

### 3.5 What to do if the reviewer's connection test fails

```
If "测试连接 / Test connection" returns red:

• Confirm the device has network. The test server is internet-reachable
  (no VPN required).
• Confirm the credentials match the Demo Account section above (we rotate
  them between submissions — the values in *this* submission are the
  authoritative ones).
• If you see a TLS error, please contact us at pendapazols42@outlook.com
  before rejecting — the test server is plain HTTPS with a valid
  Let's Encrypt cert; certificate failures usually mean the cert expired
  between submission and review.
```

### 3.6 Contact

```
Reviewer contact: pendapazols42@outlook.com
We respond within 4 business hours during the review window.
```

---

## 4. Public link / group settings

| Field | Value |
|---|---|
| External group name | Public Beta |
| Make public link available | Yes |
| Limit number of testers | 1,000 (lift later if signups exceed) |
| Allow tester feedback via TestFlight | Yes |
| Enable automatic distribution of new builds | Yes |

---

## 5. Export Compliance

```
Does your app use encryption?                              Yes
Does your app qualify for any of the exemptions?           Yes
  → Specifically: "Uses only encryption that is exempt
    under 740.17(b)" — i.e. standard HTTPS via URLSession.
  → No proprietary crypto, no algorithms beyond what iOS
    provides, no end-to-end-encrypted messaging primitives.
Add the following to Info.plist (already present in build):
  ITSAppUsesNonExemptEncryption = NO
```

Adding `ITSAppUsesNonExemptEncryption = NO` to the bundle skips the export
compliance prompt on every TestFlight upload — recommended.

---

## 6. App Privacy (Data collection disclosure)

Even for TestFlight-only builds Apple now requires the App Privacy survey.

| Question | Answer |
|---|---|
| Do you or your third-party partners collect data from this app? | **No** |

Justification (keep on file, paste into ASC if asked):

```
UniClipboard does not collect, transmit, or store any user data on our
servers. There are no first-party servers. All clipboard content, server
URLs, and credentials are stored locally on-device (UserDefaults inside the
App Group) and sent only to the user-configured SyncClipboard server. The
app embeds no analytics SDK, no crash reporter, no ad network, no tag
manager. The only outbound traffic is the user's own HTTP/HTTPS requests
to the server they typed in.
```

---

## 7. Localized App Store Connect copy (filled per locale)

### 7.1 zh-Hans (primary)

| Field | Value |
|---|---|
| App Name | UniClipboard |
| Subtitle (max 30) | 自托管跨设备剪贴板同步 |
| Promotional Text (max 170) | 兼容 SyncClipboard 协议的自托管剪贴板:文本、图片、文件全部走你自己的服务器。支持多服务器、按 Wi-Fi 自动切换、系统共享扩展、扫码配对。 |
| Keywords (max 100, comma-sep) | 剪贴板,同步,SyncClipboard,自托管,跨设备,分享,Clipboard,Self-hosted |
| Support URL | https://github.com/mkdir700/uniclipboard |
| Marketing URL (optional) | _to fill if landing page goes live_ |
| Privacy Policy URL | https://uniclipboard.app/privacy (must be live before submission) |

### 7.2 en

| Field | Value |
|---|---|
| App Name | UniClipboard |
| Subtitle | Self-hosted clipboard sync |
| Promotional Text | Self-hosted clipboard sync that speaks the SyncClipboard protocol. Text, images, and files stay on your own server. Multi-server, Wi-Fi auto-switch, share extension, QR pairing — all built in. |
| Keywords | clipboard,sync,SyncClipboard,self-hosted,cross-device,share,paste,LAN |
| Support URL | https://github.com/mkdir700/uniclipboard |
| Privacy Policy URL | https://uniclipboard.app/privacy |

---

## 8. Pre-submission checklist

Run through this list immediately before clicking "Submit for Review" on the
External group. Each item has bitten us (or a comparable project) at least
once.

- [ ] Build uploaded via Xcode Organizer is **release-signed** with the
      distribution certificate, not development.
- [ ] `ITSAppUsesNonExemptEncryption = NO` is in the built Info.plist
      (`/usr/libexec/PlistBuddy -c "Print :ITSAppUsesNonExemptEncryption"
      "$INFOPLIST_PATH"`).
- [ ] The marketing version + build number on this page match the uploaded
      build (`agvtool what-version` / `what-marketing-version`).
- [ ] Demo Account server is up; `curl -u reviewer:<pass>
      https://beta-stub.uniclipboard.app/SyncClipboard.json` returns 200.
- [ ] Privacy Policy URL returns 200 and renders.
- [ ] Support URL returns 200.
- [ ] Both `zh-Hans` and `en` localizations are filled in App Store Connect
      (a missing locale will reject the External submission outright).
- [ ] App icon set is complete (1024×1024 marketing icon present).
- [ ] No console-log noise on launch when running the release config
      (`os_log` is fine; `print` statements should be gone).
- [ ] Beta App Review Information section in ASC carries the full block
      from §3.2 + §3.3 + §3.4 + §3.5 — Apple's reviewer reads this verbatim.
- [ ] Internal group has tested this exact build for >=24h with no P0
      crashes.
- [ ] Crash-free sessions in TestFlight Internal >= 99% for the candidate
      build.

---

## 9. Post-submission

| Outcome | Next step |
|---|---|
| Approved | External group goes live automatically. Share the public link. |
| Rejected — missing demo account | Re-check §3.1; the server was almost certainly down. |
| Rejected — 5.1.1 (privacy) | Privacy policy URL was 404 or didn't actually disclose what we collect — fix and re-submit. |
| Rejected — 2.5.1 (private API) | Should not happen — we use no private API. If it does, search the build for any reference and remove. |
| Metadata Rejected | Edit copy in ASC; metadata rejections don't require a new build. |
