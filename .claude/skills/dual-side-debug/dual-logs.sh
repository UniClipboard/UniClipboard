#!/usr/bin/env bash
# dual-logs.sh — Helper for inspecting macOS + Windows uniclipboard logs side-by-side.
#
# Logs are JSONL (one JSON object per line). Since the platform-log-dir split,
# files are written per role: uniclipboard-{gui,daemon,cli}.json.YYYY-MM-DD
# (daily rotation, UTC dates), so "today" here means UTC today. The legacy
# single-file name (uniclipboard.json.YYYY-MM-DD) is still matched for old logs.
# "Latest" = newest by mtime across roles, which in practice is the busiest
# process (usually the daemon). For per-role single-host digging, use the
# `local-log-debug` skill instead.
#
# macOS path:    $MAC_BASE/app.uniclipboard.desktop[-<UC_PROFILE>]/
#                (Apple convention: ~/Library/Logs/<app>; the app dir IS the log
#                dir — there is NO `logs/` subdir on macOS anymore)
# Windows path:  $WIN_BASE/app.uniclipboard.desktop[-<WIN_PROFILE>]/logs/
#                (SMB share of //<host>/Users/<user>/AppData/Local mounted at
#                $WIN_BASE; Windows keeps the `logs/` subdir under the data root)
#
# Win profile resolution priority:
#   1. --win-profile <name> on the command line
#   2. $WIN_LOGS env var (legacy full-path override, skips $WIN_BASE)
#   3. auto-detect: profile under $WIN_BASE whose latest log file has the newest mtime

set -euo pipefail

MAC_BASE="${MAC_BASE:-$HOME/Library/Logs}"
WIN_BASE="${WIN_BASE:-/tmp/win-local}"
WIN_LOGS_OVERRIDE="${WIN_LOGS:-}"
DEFAULT_PROFILE="${UC_PROFILE_DEFAULT:-dev}"

usage() {
  cat <<'EOF'
Usage: dual-logs.sh <command> [options]

Commands:
  status                       Show both sides: profile dirs, latest log files, freshness.
                               Windows profile is auto-detected (newest mtime) unless overridden.
  list-profiles [--side mac|win|both]
                               List all profile dirs (default: both sides).
  paths  [--profile <name>] [--win-profile <name>]
                               Print resolved log file paths for both sides.
  tail   [--profile <name>] [--win-profile <name>] [--lines N] [--side mac|win|both]
                               Tail latest log file. Default: --side both --lines 50.
  query  [--profile <name>] [--win-profile <name>] [--side mac|win|both]
                               Pipe latest logs through jq. Read --filter from arg or stdin.
                               Examples:
                                 dual-logs.sh query --filter '. | select(.level=="ERROR")'
                                 dual-logs.sh query --side mac --filter '. | select(.target|test("pairing"))'
  merge  [--profile <name>] [--win-profile <name>] [--since <ISO8601>] [--lines N]
                               Merge both sides chronologically by .timestamp.
                               Each line gets a .side field ("mac" or "win") prepended.
  grep   <pattern> [--profile <name>] [--win-profile <name>] [--side mac|win|both]
                               Plain-text grep on the latest log files.

Profile resolution:
  --profile <name>             Selects mac profile dir: app.uniclipboard.desktop[-<name>]
                               Default: $UC_PROFILE_DEFAULT (currently: dev)
  --win-profile <name>         Selects win profile under $WIN_BASE the same way.
                               Default: auto-detect newest mtime under $WIN_BASE.
                               Use "default" for the no-suffix dir (app.uniclipboard.desktop).
  WIN_LOGS=...                 Env override that bypasses $WIN_BASE entirely and treats the
                               value as the literal logs/ dir. Useful for ad-hoc mounts.

Notes:
  * Log file names use UTC dates. "Latest" = newest by mtime, not by clock date.
  * If mtime is far behind current time, the profile is likely wrong — ask the user.
EOF
}

# --- generic profile resolution -------------------------------------------

# profile_dir <base> <profile> -> "<base>/app.uniclipboard.desktop[-<profile>]"
profile_dir() {
  local base="$1" profile="${2:-}"
  if [[ -z "$profile" || "$profile" == "default" ]]; then
    echo "$base/app.uniclipboard.desktop"
  else
    echo "$base/app.uniclipboard.desktop-$profile"
  fi
}

mac_profile_dir() { profile_dir "$MAC_BASE" "${1:-}"; }
win_profile_dir() { profile_dir "$WIN_BASE" "${1:-}"; }

# Resolve the actual logs directory per platform. macOS logs live directly under
# ~/Library/Logs/<app> (no subdir); Windows keeps a `logs/` subdir under the
# data-local app root. These are the asymmetry the rest of the script relies on.
mac_logs_dir() { mac_profile_dir "${1:-}"; }
win_logs_dir() { printf '%s/logs' "$(win_profile_dir "${1:-}")"; }

# list_profile_dirs_in <base> [logs_subdir]
#   Print one line per existing profile dir that has logs. `logs_subdir` is the
#   relative dir holding the log files: "logs" for Windows (default, keeps the
#   data-root subdir), "" for macOS (the profile dir itself is the log dir).
#   Format: <profile>\t<logdir>\t<latest_log>\t<latest_mtime_iso>\t<latest_mtime_epoch>
list_profile_dirs_in() {
  local base="$1" logs_subdir="${2-logs}"
  shopt -s nullglob
  local d name profile logdir latest mtime epoch
  for d in "$base"/app.uniclipboard.desktop "$base"/app.uniclipboard.desktop-*; do
    if [[ -n "$logs_subdir" ]]; then logdir="$d/$logs_subdir"; else logdir="$d"; fi
    [[ -d "$logdir" ]] || continue
    name="$(basename "$d")"
    if [[ "$name" == "app.uniclipboard.desktop" ]]; then
      profile="default"
    else
      profile="${name#app.uniclipboard.desktop-}"
    fi
    latest="$(latest_log_in "$logdir" || true)"
    if [[ -n "$latest" ]]; then
      mtime="$(stat -f '%Sm' -t '%Y-%m-%dT%H:%M:%S' "$latest" 2>/dev/null || echo '')"
      epoch="$(stat -f '%m' "$latest" 2>/dev/null || echo '0')"
    else
      mtime=""
      epoch="0"
    fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$profile" "$logdir" "${latest:-<empty>}" "${mtime:-<no-logs>}" "$epoch"
  done
}

# auto_detect_profile <base> -> echoes the profile name with newest log mtime (or empty)
auto_detect_profile() {
  local base="$1"
  [[ -d "$base" ]] || { echo ""; return; }
  list_profile_dirs_in "$base" | sort -t$'\t' -k5 -nr | head -1 | cut -f1
}

latest_log_in() {
  local dir="$1"
  [[ -d "$dir" ]] || return 1
  # Pick the most recently modified log file across all roles. The glob
  # `uniclipboard*.json.*` matches the per-role names
  # (uniclipboard-{gui,daemon,cli}.json.<date>) AND the legacy single-file name
  # (uniclipboard.json.<date>). ls -t orders by mtime descending, so the busiest
  # role's file wins — usually the daemon for sync/pairing/transfer debugging.
  local f
  f="$(ls -t "$dir"/uniclipboard*.json.* 2>/dev/null | head -n1 || true)"
  # Guard against nullglob (list_profile_dirs_in enables it): with no match the
  # glob vanishes and bare `ls -t` would list the cwd. Only accept a path that
  # actually lives inside $dir.
  [[ -n "$f" && "$f" == "$dir/"* ]] || return 1
  printf '%s' "$f"
}

# --- helpers ---------------------------------------------------------------

iso_now() { date -u "+%Y-%m-%dT%H:%M:%SZ"; }

mtime_iso() { stat -f '%Sm' -t '%Y-%m-%dT%H:%M:%S' "$1" 2>/dev/null || echo ""; }

age_seconds() {
  local f="$1"
  [[ -f "$f" ]] || { echo ""; return; }
  local now mt
  now="$(date +%s)"
  mt="$(stat -f '%m' "$f")"
  echo $(( now - mt ))
}

freshness_label() {
  # Map age in seconds to a human label.
  local s="$1"
  [[ -z "$s" ]] && { echo "unknown"; return; }
  if   (( s < 120     )); then echo "live (<2m)"
  elif (( s < 600     )); then echo "recent (<10m)"
  elif (( s < 3600    )); then echo "stale (<1h)"
  elif (( s < 86400   )); then echo "old (<1d)"
  else                          echo "cold (>1d)"
  fi
}

# Resolve the windows logs dir given an optional --win-profile.
# Sets globals WIN_LOGS_RESOLVED and WIN_PROFILE_RESOLVED.
# WIN_PROFILE_RESOLVED uses "default" for the no-suffix dir, "<override>" for WIN_LOGS env,
# or the auto-detected profile name.
resolve_win_logs() {
  local win_profile="${1:-}"

  # Priority 1: explicit --win-profile flag
  if [[ -n "$win_profile" ]]; then
    WIN_PROFILE_RESOLVED="$win_profile"
    WIN_LOGS_RESOLVED="$(win_logs_dir "$win_profile")"
    return
  fi

  # Priority 2: legacy WIN_LOGS env override
  if [[ -n "$WIN_LOGS_OVERRIDE" ]]; then
    WIN_PROFILE_RESOLVED="<WIN_LOGS env>"
    WIN_LOGS_RESOLVED="$WIN_LOGS_OVERRIDE"
    return
  fi

  # Priority 3: auto-detect under $WIN_BASE
  local detected
  detected="$(auto_detect_profile "$WIN_BASE")"
  if [[ -n "$detected" ]]; then
    WIN_PROFILE_RESOLVED="$detected (auto)"
    WIN_LOGS_RESOLVED="$(win_logs_dir "$detected")"
  else
    WIN_PROFILE_RESOLVED="<none>"
    WIN_LOGS_RESOLVED=""
  fi
}

# --- commands --------------------------------------------------------------

cmd_status() {
  local profile="$DEFAULT_PROFILE" win_profile=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile)     profile="$2";     shift 2;;
      --win-profile) win_profile="$2"; shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  local mac_dir mac_log win_log
  mac_dir="$(mac_logs_dir "$profile")"

  echo "=== uniclipboard dual-log status ==="
  echo "now (local):   $(date '+%Y-%m-%d %H:%M:%S %Z')"
  echo "now (UTC):     $(iso_now)"
  echo "mac profile:   $profile"
  echo

  echo "[mac] $mac_dir"
  if [[ -d "$mac_dir" ]]; then
    mac_log="$(latest_log_in "$mac_dir" || true)"
    if [[ -n "$mac_log" ]]; then
      local age; age="$(age_seconds "$mac_log")"
      printf '  latest: %s\n  mtime:  %s   freshness: %s\n' \
        "$(basename "$mac_log")" "$(mtime_iso "$mac_log")" "$(freshness_label "$age")"
    else
      echo "  (no log files in this profile)"
    fi
  else
    echo "  (profile dir does not exist — likely wrong UC_PROFILE)"
    echo "  available profiles:"
    list_profile_dirs_in "$MAC_BASE" "" \
      | awk -F'\t' '{printf "    - %s  (latest=%s, mtime=%s)\n", $1, $3, $4}'
  fi
  echo

  resolve_win_logs "$win_profile"
  echo "[win] $WIN_LOGS_RESOLVED"
  echo "      profile: $WIN_PROFILE_RESOLVED   base: $WIN_BASE"
  if [[ -n "$WIN_LOGS_RESOLVED" && -d "$WIN_LOGS_RESOLVED" ]]; then
    win_log="$(latest_log_in "$WIN_LOGS_RESOLVED" || true)"
    if [[ -n "$win_log" ]]; then
      local age; age="$(age_seconds "$win_log")"
      printf '  latest: %s\n  mtime:  %s   freshness: %s\n' \
        "$(basename "$win_log")" "$(mtime_iso "$win_log")" "$(freshness_label "$age")"
    else
      echo "  (no log files — Windows side may not be running)"
    fi
  elif [[ ! -d "$WIN_BASE" ]]; then
    echo "  (mount missing: $WIN_BASE — re-mount the SMB share)"
  else
    echo "  (no uniclipboard profile dirs under $WIN_BASE)"
  fi

  # If we auto-detected, also show the alternatives so the user can sanity-check.
  if [[ "$WIN_PROFILE_RESOLVED" == *"(auto)"* ]]; then
    echo "  available win profiles (by mtime, newest first):"
    list_profile_dirs_in "$WIN_BASE" \
      | sort -t$'\t' -k5 -nr \
      | awk -F'\t' '{printf "    - %s  (latest=%s, mtime=%s)\n", $1, $3, $4}'
  fi
}

cmd_list_profiles() {
  local side="both"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --side) side="$2"; shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  if [[ "$side" == "mac" || "$side" == "both" ]]; then
    echo "===== mac profiles (base: $MAC_BASE) ====="
    printf '%s\t%s\t%s\t%s\n' "PROFILE" "DIR" "LATEST" "MTIME"
    list_profile_dirs_in "$MAC_BASE" "" | cut -f1-4
  fi
  if [[ "$side" == "win" || "$side" == "both" ]]; then
    [[ "$side" == "both" ]] && echo
    echo "===== win profiles (base: $WIN_BASE) ====="
    if [[ -d "$WIN_BASE" ]]; then
      printf '%s\t%s\t%s\t%s\n' "PROFILE" "DIR" "LATEST" "MTIME"
      list_profile_dirs_in "$WIN_BASE" | sort -t$'\t' -k5 -nr | cut -f1-4
    else
      echo "(mount missing: $WIN_BASE)"
    fi
  fi
}

cmd_paths() {
  local profile="$DEFAULT_PROFILE" win_profile=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile)     profile="$2";     shift 2;;
      --win-profile) win_profile="$2"; shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  local mac_dir mac_log win_log
  mac_dir="$(mac_logs_dir "$profile")"
  mac_log="$(latest_log_in "$mac_dir" 2>/dev/null || true)"
  resolve_win_logs "$win_profile"
  win_log="$(latest_log_in "$WIN_LOGS_RESOLVED" 2>/dev/null || true)"
  echo "MAC=${mac_log:-<missing>}"
  echo "WIN=${win_log:-<missing>}   (profile: $WIN_PROFILE_RESOLVED)"
}

resolve_pair() {
  # Sets globals MAC_LOG and WIN_LOG. Empties them if missing.
  local profile="$1" win_profile="${2:-}"
  local mac_dir
  mac_dir="$(mac_logs_dir "$profile")"
  MAC_LOG="$(latest_log_in "$mac_dir" 2>/dev/null || true)"
  resolve_win_logs "$win_profile"
  WIN_LOG="$(latest_log_in "$WIN_LOGS_RESOLVED" 2>/dev/null || true)"
}

cmd_tail() {
  local profile="$DEFAULT_PROFILE" win_profile="" lines=50 side="both"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile)     profile="$2";     shift 2;;
      --win-profile) win_profile="$2"; shift 2;;
      --lines)       lines="$2";       shift 2;;
      --side)        side="$2";        shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  resolve_pair "$profile" "$win_profile"
  if [[ "$side" == "mac" || "$side" == "both" ]]; then
    echo "===== [mac] ${MAC_LOG:-<missing>} ====="
    [[ -n "$MAC_LOG" ]] && tail -n "$lines" "$MAC_LOG"
  fi
  if [[ "$side" == "win" || "$side" == "both" ]]; then
    echo "===== [win] ${WIN_LOG:-<missing>}   (profile: $WIN_PROFILE_RESOLVED) ====="
    [[ -n "$WIN_LOG" ]] && tail -n "$lines" "$WIN_LOG"
  fi
}

cmd_query() {
  local profile="$DEFAULT_PROFILE" win_profile="" side="both" filter=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile)     profile="$2";     shift 2;;
      --win-profile) win_profile="$2"; shift 2;;
      --side)        side="$2";        shift 2;;
      --filter)      filter="$2";      shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  if [[ -z "$filter" ]]; then
    if [[ -t 0 ]]; then
      echo "error: --filter required (or pipe jq filter via stdin)" >&2
      exit 2
    fi
    filter="$(cat)"
  fi
  resolve_pair "$profile" "$win_profile"
  if [[ "$side" == "mac" || "$side" == "both" ]]; then
    if [[ -n "$MAC_LOG" ]]; then
      echo "===== [mac] $MAC_LOG ====="
      jq -c "$filter" "$MAC_LOG" || true
    fi
  fi
  if [[ "$side" == "win" || "$side" == "both" ]]; then
    if [[ -n "$WIN_LOG" ]]; then
      echo "===== [win] $WIN_LOG   (profile: $WIN_PROFILE_RESOLVED) ====="
      jq -c "$filter" "$WIN_LOG" || true
    fi
  fi
}

cmd_merge() {
  local profile="$DEFAULT_PROFILE" win_profile="" since="" lines=200
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile)     profile="$2";     shift 2;;
      --win-profile) win_profile="$2"; shift 2;;
      --since)       since="$2";       shift 2;;
      --lines)       lines="$2";       shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  resolve_pair "$profile" "$win_profile"
  [[ -n "$MAC_LOG" || -n "$WIN_LOG" ]] || { echo "no logs found"; exit 1; }

  local since_filter="."
  if [[ -n "$since" ]]; then
    since_filter="select(.timestamp >= \"$since\")"
  fi

  {
    [[ -n "$MAC_LOG" ]] && jq -c ". + {side:\"mac\"} | $since_filter" "$MAC_LOG"
    [[ -n "$WIN_LOG" ]] && jq -c ". + {side:\"win\"} | $since_filter" "$WIN_LOG"
  } | jq -s -c 'sort_by(.timestamp) | .[]' \
    | tail -n "$lines"
}

cmd_grep() {
  local pattern="$1"; shift || { echo "pattern required" >&2; exit 2; }
  local profile="$DEFAULT_PROFILE" win_profile="" side="both"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile)     profile="$2";     shift 2;;
      --win-profile) win_profile="$2"; shift 2;;
      --side)        side="$2";        shift 2;;
      *) echo "unknown arg: $1" >&2; exit 2;;
    esac
  done
  resolve_pair "$profile" "$win_profile"
  if [[ "$side" == "mac" || "$side" == "both" ]] && [[ -n "$MAC_LOG" ]]; then
    echo "===== [mac] $MAC_LOG ====="
    grep -F -- "$pattern" "$MAC_LOG" || true
  fi
  if [[ "$side" == "win" || "$side" == "both" ]] && [[ -n "$WIN_LOG" ]]; then
    echo "===== [win] $WIN_LOG   (profile: $WIN_PROFILE_RESOLVED) ====="
    grep -F -- "$pattern" "$WIN_LOG" || true
  fi
}

# --- dispatch --------------------------------------------------------------

cmd="${1:-}"; shift || true
case "$cmd" in
  status)        cmd_status "$@";;
  list-profiles) cmd_list_profiles "$@";;
  paths)         cmd_paths "$@";;
  tail)          cmd_tail "$@";;
  query)         cmd_query "$@";;
  merge)         cmd_merge "$@";;
  grep)          cmd_grep "$@";;
  -h|--help|help|"") usage;;
  *) echo "unknown command: $cmd" >&2; usage; exit 2;;
esac
