# SyncClipboard Shortcut template: `uniclipboard://connect` support

**Audience**: the maintainer of the SyncClipboard "Clipboard EX" iCloud-shared
Shortcut template (linked from `SYNC_CLIPBOARD_EX_INSTALL_URL`).
**Source of truth for the wire protocol**: `docs/architecture/mobile-sync-connect-uri.md`.
This document tells you how to teach the existing template to recognize the new
`uniclipboard://connect?…` URI, without throwing away the manual-three-field flow that
existing users rely on.

---

## 1. Why this guide exists

The desktop app has two onboarding affordances for iOS users:

1. The **native iOS App** (`app.uniclipboard.UniClipboard`) registers the
   `uniclipboard://` URL scheme and handles connect URIs through `.onOpenURL`. This is
   the primary path going forward (see sibling guide `ios-app-connect-uri.md`).
2. The **SyncClipboard Shortcut template** is a **fallback for users who don't have
   the native App installed**. The desktop's credential modal continues to surface the
   iCloud install link in a secondary "first time? install the shortcut" card.

Until phase 2 of issue #789, the desktop QR encoded the iCloud install URL. The
existing Shortcut template doesn't read the QR at all — it asks the user to type three
fields by hand. From phase 2 onward, the desktop QR encodes a `uniclipboard://connect`
URI carrying url / user / pwd. To keep the fallback path useful, the Shortcut template
must learn to:

- Accept the URI as input (when the user opens it from Safari or via iOS smart
  actions, the Shortcut should be one of the offered handlers — or the user manually
  runs it with the URI on the clipboard).
- base64url-decode the `p` query parameter.
- Write the resulting `url` / `user` / `pwd` into the SyncClipboard configuration the
  template already manages.

The user still does steps 1–2 of the existing template (server selection / first-run
init); only the "type three fields" step is automated away.

> If you've migrated all your users to the native App and are willing to break the
> Shortcut fallback path, you can skip this entire document — just note in your
> release notes that the iCloud install link is now a legacy entry point.

---

## 2. Two-phase UX (unchanged)

1. **First time only**: user opens `SYNC_CLIPBOARD_EX_INSTALL_URL` in iPhone Safari →
   "Get Shortcut" → the template lands in the Shortcuts app.
2. **Every add-device thereafter**: from the desktop credential modal, the user uses
   either the system Camera on the QR (preferred — direct route to native App or
   Shortcut) or copies the connect URI string and hands it to the template.

This guide covers only the additions to the template. It does not re-document the
SyncClipboard `GET /SyncClipboard.json` polling actions or the "save to keychain"
steps the template already implements.

---

## 3. Actions to add to the template

Edit the template in the Shortcuts editor (macOS or iPhone). The block below is a
self-contained subroutine; place it **before** the existing "Set URL/User/Password
keychain values" steps and disable those manual prompts when the subroutine fires.

> Each row below is one Shortcut action. "→ Variable X" means the action's result is
> magic-variable-bound under that name; later actions reference X.

| #   | Action                | Configuration                                                                                              | → Variable          |
| --- | --------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------- |
| 1   | **Receive Input**     | Accepts: URLs. Top of the Shortcut, marked "Show in Share Sheet"                                           | `ShortcutInput`     |
| 2   | **If**                | `ShortcutInput` `contains` `uniclipboard://connect`                                                        | (branch start)      |
| 3   | **Get URLs from Input** | Input: `ShortcutInput`                                                                                   | `RawURL`            |
| 4   | **URL Encode**        | Mode: **Decode** · Input: `RawURL`                                                                         | `DecodedURL`        |
| 5   | **Get Component of URL** | Component: `Query` · URL: `DecodedURL`                                                                  | `Query`             |
| 6   | **Match Text**        | Pattern: `p=([^&]+)` · Input: `Query`                                                                      | `Matches`           |
| 7   | **Get Group from Matched Text** | Group Index: `1`                                                                                 | `PEncoded`          |
| 8   | **Replace Text**      | Find: `-` · Replace: `+` · Input: `PEncoded`                                                              | `Step1`             |
| 9   | **Replace Text**      | Find: `_` · Replace: `/` · Input: `Step1`                                                                | `Step2`             |
| 10  | **Count**             | Items in: characters of `Step2`                                                                            | `Len`               |
| 11  | **Calculate**         | `(4 - (Len mod 4)) mod 4`                                                                                  | `PadCount`          |
| 12  | **Repeat**            | `PadCount` times → **Get Variable** `Step2` → **Combine Text** with `=` → set `Step2`                     | `PaddedB64`         |
| 13  | **Base64 Encode**     | Mode: **Decode** · Input: `PaddedB64`                                                                      | `JSONBytes`         |
| 14  | **Get Text from Input** | Encoding: UTF-8 · Input: `JSONBytes`                                                                    | `JSONString`        |
| 15  | **Get Dictionary from Input** | Input: `JSONString`                                                                              | `Payload`           |
| 16  | **Get Dictionary Value** | Key: `url` · From: `Payload`                                                                            | `BaseURL`           |
| 17  | **Get Dictionary Value** | Key: `user` · From: `Payload`                                                                           | `User`              |
| 18  | **Get Dictionary Value** | Key: `pwd` · From: `Payload`                                                                            | `Pwd`               |
| 19  | (your existing "save URL/user/pwd to keychain" actions, fed by `BaseURL` / `User` / `Pwd` instead of typed text) |                                                                                       |                     |
| 20  | **Otherwise**         | (fallback for non-URI inputs — legacy manual flow)                                                         |                     |
| 21  | (existing "ask the user to type three fields" steps go here, unchanged)                                                                                |                                                                                       |                     |
| 22  | **End If**            |                                                                                                            |                     |

### Notes on individual steps

- **Step 4 (URL Encode → Decode)**: percent-decodes the URL once so `+` and `=` in the
  `p` payload survive intact. The connect URI's `p` value never contains percent-
  escapes (base64url uses `-`/`_`), but the surrounding query may, so decode at the
  whole-URL level is safe and idempotent.
- **Steps 6–7 (Match Text → Group)**: Shortcuts has no built-in "get query parameter
  by name", so we regex-extract. The pattern uses `[^&]+` (greedy up to the next
  `&` or end of string) and captures into group 1.
- **Steps 8–12 (base64url → base64)**: Shortcuts' built-in `Base64 Encode` action
  speaks **standard** base64 only — it rejects `-`/`_` and requires `=` padding to be
  present. The four micro-steps map base64url-no-pad back to standard base64 in a way
  that matches the encoder side bit-for-bit:
  - `-` → `+` and `_` → `/` (URL-safe → standard alphabet)
  - re-add `=` padding to a length that's a multiple of 4
- **Step 11 (the calculation)**: `(4 - (len mod 4)) mod 4` produces 0/1/2/3 padding
  characters. Don't shortcut to "always append `==`" — that breaks on payloads whose
  length is already a multiple of 4.
- **Step 15 (Get Dictionary from Input)**: Shortcuts parses the UTF-8 JSON string into
  a dictionary in one action. The encoder writes minified JSON with fields in spec
  order, but the parser doesn't care about order — only that the four required keys
  (`url`, `user`, `pwd`, and `v=1`) exist.
- **`v` and `svc` validation**: omitted from the subroutine on purpose. The Shortcut
  is a best-effort fallback; the desktop is the only QR issuer and it will only emit
  v1 / `svc=mobile-sync` until a coordinated rollout. Adding strict validation here
  just adds Shortcut nodes that will mostly be dead code. Re-add if the protocol
  evolves.

---

## 4. Verifying with the golden vector

Spec §7.1 publishes one happy-path URI that all conformant decoders agree on. Use it
to dry-run the Shortcut without touching the desktop:

```
uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ
```

After running the Shortcut with this URI, the bound variables MUST equal:

- `BaseURL` = `http://192.168.1.5:42720`
- `User` = `mobile_aabbccdd`
- `Pwd` = `AbCdEfGhIjKlMnOpQrSt`

If any of the three drift, walk the actions in this order — they are the high-risk
ones:

1. **Step 7** (Get Group): make sure group index is `1`, not `0`. Group 0 is the full
   match including the `p=` prefix.
2. **Steps 8–9** (Replace Text): off-by-one — confirm the source `Step2` cascade so
   the second Replace consumes the first's output, not the original `PEncoded`.
3. **Step 13** (Base64 Decode): if this action errors, padding was wrong; verify
   `PadCount` in step 11 is 0, 1, 2, or 3 — never 4.

---

## 5. Distribution

After editing the template:

1. In the Shortcuts editor: **Share → iCloud Link**. iCloud assigns a stable URL the
   first time and **reuses** it on subsequent shares as long as you re-share from the
   same shortcut record. If iCloud issues a new link, the desktop constant
   `SYNC_CLIPBOARD_EX_INSTALL_URL` (in `src-tauri/.../register_device.rs`) must be
   updated and shipped in a desktop release.
2. Run the template once on a clean iPhone (Settings → Shortcuts → Reset → Re-install)
   and confirm both branches still work:
   - The new branch: feed the §4 golden URI from Safari's address bar.
   - The legacy branch: run the Shortcut directly with no input — verify the typed-
     three-fields flow is intact.

---

## 6. When to retire this template

Once telemetry shows >95% of new mobile-sync onboardings go through the native iOS
App's `.onOpenURL` path, this fallback can be put into maintenance mode:

- Stop adding new actions; only fix bugs.
- The desktop credential modal can hide the "first time? install the shortcut" card
  behind a "Show advanced options" disclosure to nudge users to the App.
- Eventually, drop `SYNC_CLIPBOARD_EX_INSTALL_URL` and remove the install-URL field
  from the desktop DTO (a deliberate, breaking, versioned change — not part of
  phase 4).
