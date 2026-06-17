---
name: local-log-debug
description: Inspect and analyze uniclipboard's local JSONL logs on a SINGLE machine — query, filter, and time-merge the per-role (gui/daemon/cli) log files to answer "what just happened" or trace a symptom (pairing, sync, transfer, clipboard capture, daemon lifecycle) back through the logs. Use when the user asks to "check the logs", "why did X fail", "what did the daemon do", or describes a bug to diagnose from logs on this one host. NOT for cross-device peer debugging (use dual-side-debug) and NOT for writing/reviewing tracing code (use tracing-best-practices).
user-invocable: true
---

# local-log-debug

Read and reason over uniclipboard's local logs on **one** machine, across the
three process roles (gui / daemon / cli), using the current platform-conventional
log layout.

The helper script `.claude/skills/local-log-debug/uc-logs.sh` is the **only**
thing you should need to invoke for log work — it resolves the per-platform log
directory, picks the latest file per role, and handles tail / grep / structured
query / time-merge. Don't hand-roll `ls`/`tail`/`jq` pipelines unless the script
genuinely can't express what you need.

## When this skill applies (and when it does NOT)

| Situation | Use |
| --- | --- |
| "Check the logs on this machine", trace a symptom from local logs | **this skill** |
| Cross-device: "Windows didn't receive…", "Mac sent but peer…" | `dual-side-debug` (two hosts, SMB-mounted peer) |
| Writing / reviewing `#[instrument]`, `tracing::*`, subscriber setup | `tracing-best-practices` |
| Bundling recent logs to hand off / attach to an issue | `uniclip debug export-logs` (CLI) — see below |
| Build / cargo / typecheck failures | not these JSONL logs at all |
| Daemon HTTP API behavior or sqlite state | logs are observability, not state |

## Log layout you must remember (current, post-split)

Single source of truth for *where logs live* is `uc_app_paths::app_log_dir()`.
Logs are **separate from the data root** (since the platform-log-dir split) and
are written **per role** so co-resident processes never share a file.

* Directory (`<app>` = `app.uniclipboard.desktop[-<UC_PROFILE>]`):
  * **macOS**: `~/Library/Logs/<app>/`
  * **Linux**: `$XDG_STATE_HOME/<app>/logs/` (default `~/.local/state/...`; falls back to the data-local root)
  * **Windows**: `%LOCALAPPDATA%\<app>\logs\`
  * **portable build**: `<exe>/data/logs/`
* Per-role files, daily rotation, **7-day retention** (older pruned on start):
  * `uniclipboard-gui.json.<UTC-date>` — the Tauri GUI host (`uniclipboard`)
  * `uniclipboard-daemon.json.<UTC-date>` — the detached `uniclipd` daemon
  * `uniclipboard-cli.json.<UTC-date>` — the `uniclip` CLI
* Format: **JSON Lines**. Every line has `timestamp` (UTC ISO-8601, ends `Z`, and
  is always the **first** field), `level`, `target`, `message`, usually `span`
  and `device_id`, plus flattened span/event fields.
* The date in the filename is **UTC**, not local time. A `...2026-06-17` file can
  be the live one while it's still 2026-06-16 in your local evening.

> ⚠️ Do not trust the older layout. The `dual-side-debug` skill and some legacy
> data roots still reference `~/Library/Application Support/.../logs/uniclipboard.json`
> (single file, no role). That is the **pre-split** layout — wrong for this skill.

### Profile resolution

The app dir gets a `-<profile>` suffix from `UC_PROFILE`. The local dev default
is **`dev`** (`package.json`'s `tauri:dev` sets `UC_PROFILE=dev`), so the script
assumes `dev`. Override with `--profile <name>`, or `--profile default` for the
no-suffix `app.uniclipboard.desktop` dir.

### Escape hatch: `UC_LOG_DIR`

If the script's platform/profile resolution ever disagrees with reality
(portable build, WSL reading a Windows app's logs under `/mnt/c/...`, an
unusual mount), set `UC_LOG_DIR=/abs/path/to/logs` and the script uses it
verbatim. This is also the fallback for a pure-PowerShell host where bash can't
run: resolve the dir per the table above and read the JSONL with your own tools.

## Always run `status` first

Before answering anything that depends on log content:

```bash
.claude/skills/local-log-debug/uc-logs.sh status
```

Then judge:

1. Does the resolved log dir exist? If not, you're probably on the wrong
   profile — try `--profile default`, or check sibling dirs in the parent.
2. For each role, is the latest file `live (<2m)` / `recent (<10m)`? Anything
   `stale` / `old` / `cold` means that process likely isn't running right now —
   you may be reading a frozen session.

If the user is actively reproducing but the relevant role is `cold`/`(no file)`,
**stop and confirm the profile** before drawing conclusions. Don't silently
analyze stale logs.

## Commands

Always invoke via the script, from the repo root:

```bash
.claude/skills/local-log-debug/uc-logs.sh <command> [args] [flags]
```

| Command | When to use |
| --- | --- |
| `status` | First call of any session. Profile + resolved dir + per-role freshness. |
| `paths`  | Just need the resolved dir and latest file per role (to feed another tool). |
| `tail`   | Quick "what just happened" — last 50 lines per selected role. |
| `grep <pattern>` | Plain string match (a device id, an error fragment, a request id). Cheap first probe. |
| `query --filter '<jq>'` | Structured jq filter over the JSONL. Best for level / target / span filters. |
| `merge`  | Time-interleave the selected roles into one chronological stream, injecting `.role`. Use whenever the question is *"what happened across gui ↔ daemon around time X"*. |

### Flags

* `--role gui\|daemon\|cli\|all` — restrict to a role (default `all`).
* `--profile <name>` — profile override (default `dev`; `default` = no suffix).
* `--lines N` — output cap for tail/grep/query (default 50); per-file scan depth for merge (default 2000).
* `--since <ISO8601>` — merge only: drop lines older than this UTC timestamp.

## Recommended workflow

1. **Ground yourself.** Run `status`. Confirm the right profile and that the
   relevant role is live. Resolve any profile mismatch with the user first.
2. **Narrow the time window.** Ask when the issue reproduced (or read it from the
   conversation), convert to UTC, and pass it to `merge --since`.
3. **Start broad, then narrow.**
   * Broad cross-role story: `merge --since <UTC> --lines 800`.
   * Narrow by signal: `query --filter '.level=="ERROR" or .level=="WARN"'`, or
     by `target` / `span` for the suspect subsystem.
4. **Correlate by id, don't assume.** When the symptom spans roles ("GUI asked,
   daemon never acted"), grep the **same id** (`request_id`, `transfer_id`,
   `entry_id`, `device_id`, `session_token_jti`) across roles — the merged view
   beats two parallel monologues.
5. **Quote sparingly.** Logs are noisy. In your reply, quote the 3–10 lines that
   carry signal, with role + timestamp. Never dump raw JSONL walls.

## jq cookbook

Plug these straight into `query --filter '<jq>'`:

```jq
# Errors and warnings only
.level == "ERROR" or .level == "WARN"

# One subsystem (substring match on target)
.target | test("pairing|file_transfer|clipboard_sync")

# A specific span chain
.span | test("daemon.ws.connection")

# Around a particular id (works for any flattened field)
.request_id == "b6c8588aa8a8699c"
.device_id == "acf2b4da-a6f9-4b77-9074-e13b0ffc9496"
```

Compact projection for human reading (pipe `query`/`merge` output through this):

```bash
... | jq -c '{ts:.timestamp, role:.role, lvl:.level, tgt:.target, span:.span, msg:.message}'
```

### Subsystem target / span cheat-sheet

Real strings seen in these logs, to seed your `target`/`span` filters:

| Subsystem | Representative `target` / `span` |
| --- | --- |
| Daemon HTTP/WS API | `uc_webserver::api::server`, `uc_webserver::api::ws`, span `daemon.ws.connection`, `uc_webserver::api::control_lease` |
| Daemon lifecycle / workers | `uc_daemon::daemon::app`, `uc_daemon::daemon::workers::*` (clipboard_watcher, peer_keepalive, file_sync_orchestrator, inbound_clipboard_sync) |
| Clipboard capture (Linux) | `uc_platform::clipboard::platform::linux::x11::event_loop` (Wayland under the sibling `wayland` path) |
| Sync / transfer (app layer) | `uc_application::usecases::clipboard_sync`, `uc_application::usecases::file_transfer`, `uc_application::usecases::mobile_sync`, span `peer.dispatch` |
| Pairing / setup | `uc_application::facade::space_setup`, `uc_application::usecases::pairing_*` |
| Search | `uc_application::facade::search::coordinator`, span `search.run_manual_rebuild_now` |
| iroh networking (noisy) | `iroh::net_report`, `iroh::magicsock`, `iroh_relay::ping_tracker`, `iroh_blobs::store::*` |

Note: a lot of `iroh*` / relay traffic is infra noise. The log profiles already
mute the worst of it; when scanning, prefer filtering to `uc_*` targets first.

## Verbosity is controlled elsewhere

Local log **level** comes from `RUST_LOG` > `UC_LOG_PROFILE` > build type
(debug→`Dev`, release→`Prod`). If a subsystem's lines are missing entirely, the
profile may be filtering them — that's a *capture* problem, not a *reading*
problem, and belongs to how the app was launched (e.g. `uniclip debug on`,
`UC_LOG_PROFILE=debug_clipboard`), not to this skill.

## Relationship to `uniclip debug export-logs`

The CLI can bundle recent logs for handoff:

```bash
uniclip debug export-logs [--since-hours N]   # default 24h; writes a dir + manifest
```

Use that when the goal is to **package** logs (attach to an issue, send to the
user). Use **this skill** when the goal is to **read and reason** about them in
place. Related: `uniclip debug status|on|off` shows/toggles the effective log
profile.

## Things to avoid

* Don't `cat` whole log files — daemon files get large.
* Don't conclude "the daemon is broken" without checking its log **freshness**
  in `status` first; a `cold` file just means the daemon isn't running.
* Don't convert UTC ↔ local time silently. If you convert, say so.
* Don't edit log paths in this doc if the layout changes — fix `uc-logs.sh`
  first (and confirm against `uc_app_paths::app_log_dir()`), then this doc.
* Don't confuse this with `dual-side-debug`: that skill reads a *second host's*
  logs over a mount and still uses the *old* single-file layout.
```

