# Changelog

## 1.0 (10) — 2026-06-11

### New Features

- **Multi-URL per-server auto-switch** — a server profile now holds an ordered
  list of candidate URLs (LAN / Tailscale / WAN, auto-classified by host shape)
  and the app probes reachability to pick the best endpoint automatically on
  profile switch, network change, foreground, and sync failure. Replaces the
  old per-server Wi-Fi/cellular/Tailscale trigger strategy.
- **Multi-URL editor** with address-class chips and a probe-all 测试连接 that
  shows per-URL ✓/✗ plus a 将使用 badge for the endpoint that will be used.
- **QR pairing carries the full URL list**, so scanning a connect code imports
  every candidate address in one step.

### Bug Fixes

- Home clipboard cards now render as true 1:1 squares on every device and
  orientation instead of a fixed height.
- Top-bar capsule labels (选择 / 全选 / 完成) stay on a single line when the
  error badge crowds the bar, instead of wrapping vertically.
- Clarified the "trust insecure certificate" setting copy — it applies to
  self-signed HTTPS only; plain HTTP is unaffected.

## 1.0 (9) — 2026-06-10

### New Features

- **First-run onboarding walkthrough** — guided feature tour plus step-by-step
  setup sheets for the custom keyboard, share extension, and paste permission,
  re-viewable anytime via Settings → 功能引导.
- **Paste-style two-column card grid** on the Home page, with multi-select,
  batch share/delete, and a search button that liquid-morphs into the search
  field.
- **Rich URL link cards** — text entries that are links now render with an
  Open Graph preview image and title.
- **Immersive image cards** with a checkerboard letterbox background, backed by
  a new memory + disk thumbnail cache shared with the keyboard extension.
- **Local clipboard history** — history is now recorded even with no server
  configured or reachable, so you keep a timeline before pairing a backend.

### Bug Fixes

- Unified file/image/text hashing to content-only SHA-256 and reworked
  history re-apply so tapping an entry re-syncs it to other devices.
- Removed Liquid Glass from inside the keyboard tray to fix the translucent
  band / hairline artifact; tray height now derives from `KeyboardLayout`.
- Fixed keyboard top-bar taps being intercepted by the quiet-chrome header.
- Keyboard image-card copy now reads the local cache before refetching, and
  resolved a keyboard/app watermark coordination loop that echoed entries.

## 1.0 (8) — 2026-06-08

### New Features

- **Custom keyboard extension** — UniClip keyboard lets you paste synced clipboard
  content directly into any app without the "Allow Paste" prompt.
- **Key sound & haptic feedback** for the custom keyboard.
- **Per-server auto-switch strategy** — each server can now be assigned a Wi-Fi,
  cellular, or Tailscale trigger so the active server switches automatically based
  on network conditions.
- **Parameterized send/receive shortcuts** — Shortcuts actions now include a server
  picker, so you can target a specific server instead of always using the active one.

### Bug Fixes

- Fixed keyboard return-key glyph not visible in dark mode.
- Fixed AddServerSheet reappearing as a blank form after dismiss.

## 1.0 (7) — 2026-06-01

### New Features

- Unified "current server" concept (default server + home pin merged).
- Consent-based clipboard push (PasteButton) to eliminate the "Allow Paste" prompt.

### Bug Fixes

- Fixed text/URL extraction from PasteButton providers.
- Symmetric push/pull nudge stack refactor.

## 1.0 (6) — 2026-05-25

### New Features

- Pin server via home-screen chip, override SSID auto-switch.
- Light / dark / system theme preference.

### Bug Fixes

- Persisted last-synced hash to file to defeat cfprefsd cross-process lag.
- Detect cross-app pasteboard changes via changeCount polling.
- Allow HTTP to Tailscale CGNAT range via NSAllowsArbitraryLoads.
- Restored cross-line selection of wrapped URLs in clipboard preview.
- Faster long-text clipboard preview rendering.
