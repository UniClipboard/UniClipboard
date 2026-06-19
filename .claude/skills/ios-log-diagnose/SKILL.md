---
name: ios-log-diagnose
description: Drive the UniClipboard iOS app in a simulator and read its OSLog yourself to diagnose a mobile-sync bug, instead of asking the user to paste logs. Use when debugging why the iOS sync engine or M5 reducer did something, reproducing an iOS sync / clipboard bug on the simulator, or whenever you'd otherwise ask the user "what do the logs say". All output is Swift OSLog under subsystem `app.uniclipboard` — the Rust core (uc-mobile) emits no logs of its own; the Swift shell logs the reducer's decisions.
user-invocable: true
---

# ios-log-diagnose

Read the UniClipboard iOS app's logs **yourself**. The mobile-sync decision core lives in Rust (`uc-mobile` reducer), but it has no logger — the Swift `SyncEngine` shell logs every reducer decision and outcome to OSLog. So one OSLog stream shows both the native Swift logic and the Rust core's behavior.

iOS app repo: `/Users/mark/MyProjects/iOSApp/UniClipboard`. The reducer-path log harness lives in `UniClipboard/Sync/SyncEngine.swift` (commit `e94ffd1`).

The helper is `.claude/skills/ios-log-diagnose/ios-logs.sh` — the **only** thing to invoke. Don't hand-roll `simctl`/`log` pipelines unless it can't express what you need (macOS has no `timeout`; the script wraps `log stream` in a `perl` alarm — reproducing that by hand is the usual mistake).

## Two channels — pick by what you're after

Log level decides where a line goes. This split is the whole mental model:

* **`debug` → live only, never persisted.** The per-tick decision trace: `sync preamble: proceed/stop(...)`, `sync route: converged/server-new/push(...)`, push silent-skip. Seen ONLY by streaming while it happens. Use to watch what the reducer decides each tick.
* **`notice` / `error` → persisted, queryable after the fact.** The event trail: `sync apply/stage/push/consent-push`, `sync history: round done`, the five `handle_*` transitions, and `tick: SyncError ...`. Use to reconstruct what already happened.

So: **streaming a reproduction** answers "what is it deciding right now"; **`show`** answers "what happened in the last N minutes".

## Steps

1. **Drive** — boot a sim, install the newest build, inject the Rust-core flag ON + an active server, launch:
   ```bash
   .claude/skills/ios-log-diagnose/ios-logs.sh drive [SERVER_URL]
   ```
   The engine only ticks with an active server, so `drive` always injects one. A dead URL (the default) still exercises preamble → proceed → `getClipboard` fails → `tick: SyncError` → backoff — enough to see decisions + the error path. For the happy path (`route`/`apply`/`push`/`converge`), pass a `SERVER_URL` that actually returns data (a running `uniclipd` mobile-sync server).
   *Completion: the command prints `injected: flag=ON server=...` and a launch PID.*

2. **Read** — choose the channel:
   ```bash
   .claude/skills/ios-log-diagnose/ios-logs.sh stream [SECONDS] [CATEGORY]   # live debug, default 15s
   .claude/skills/ios-log-diagnose/ios-logs.sh show   [DURATION] [CATEGORY]  # persisted notice/error, default 5m
   ```
   `CATEGORY` (optional): `sync` (SyncEngine — reducer + tick; the usual one), `network` (HTTP client / connect-uri), `store` (persistence), `app`, `intents`. Omit for every category under `app.uniclipboard`.
   *Completion: you have the `sync` (or target) lines for the run you triggered, and can name what the reducer decided / what failed.*

## Gotchas

* **No content, ever.** Logs carry states / bools / counts / decision enums plus an 8-char `hashTag` prefix — a one-way SHA fingerprint that correlates one clipboard item across pull → stage → apply → push → converge, never the content.
* **Flag injection is forward-safe.** `drive` writes `mobileCore.syncClientUsesRustCore = YES`. Once the native A/B paths are deleted and Rust is the only path, that key just goes unread — the script keeps working unchanged.
* **App Group, not the app sandbox.** Flag + server live in `defaults` suite `group.app.uniclipboard.UniClipboard`; injecting them needs a `terminate` + `launch` so `loadServers` re-reads. `drive` handles this.
* **More injection hooks exist.** `grep ProcessInfo.processInfo.environment` in the iOS repo for `UC_*` (e.g. `UC_DEVICE_TEXT` to seed a device copy, `UC_TEST_QR_PAYLOAD` to add a server via connect-uri) — passed through `SIMCTL_CHILD_<NAME>=value` on launch.
* Build first if `drive` says "no built UniClipboard.app": `xcodebuild -scheme UniClipboard -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' build CODE_SIGNING_ALLOWED=NO`.
