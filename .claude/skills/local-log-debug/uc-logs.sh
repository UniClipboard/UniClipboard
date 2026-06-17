#!/usr/bin/env bash
# uc-logs.sh — single-machine, cross-platform reader for uniclipboard's
# per-role JSONL logs. Companion to the `local-log-debug` skill.
#
# Mirrors `uc_app_paths::app_log_dir()` (the single source of truth for where
# logs live) so it resolves the same directory the running app writes to. If the
# heuristics ever disagree with the app, override with UC_LOG_DIR and the script
# uses that path verbatim.
#
# Roles map to file stems exactly as `uc_observability::scope::role_log_file_stem()`:
#   gui    -> uniclipboard-gui.json.<UTC-date>
#   daemon -> uniclipboard-daemon.json.<UTC-date>
#   cli    -> uniclipboard-cli.json.<UTC-date>
#
# Usage:
#   uc-logs.sh status                         # profile, resolved dir, per-role freshness
#   uc-logs.sh paths                          # resolved dir + latest file per role
#   uc-logs.sh tail   [--role R] [--lines N]
#   uc-logs.sh grep   <pattern> [--role R] [--lines N]
#   uc-logs.sh query  --filter '<jq>' [--role R] [--lines N]
#   uc-logs.sh merge  [--since <ISO8601>] [--lines N]   # time-interleave roles, inject .role
#
# Flags:
#   --role gui|daemon|cli|all   (default: all)
#   --profile <name>            (default: dev; use `default` for the no-suffix dir)
#   --lines N                   (default: 50 for tail/grep/query, 2000 per-file scan for merge)
#   --since <ISO8601>           (merge only; drop lines with timestamp < this, UTC lexical compare)
#
# Env overrides:
#   UC_LOG_DIR     full path to the logs dir; bypasses all platform/profile resolution
#   UC_PROFILE     fallback profile when --profile is omitted (matches the app's env)
set -euo pipefail

APP_DIR_NAME="app.uniclipboard.desktop"

die() { echo "uc-logs: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing required tool: $1"; }

# ---- argument parsing -------------------------------------------------------
CMD="${1:-}"; shift || true
ROLE="all"
PROFILE="${UC_PROFILE:-dev}"
LINES=""
SINCE=""
FILTER=""
PATTERN=""

while [ $# -gt 0 ]; do
  case "$1" in
    --role)    ROLE="${2:?--role needs a value}"; shift 2 ;;
    --profile) PROFILE="${2:?--profile needs a value}"; shift 2 ;;
    --lines)   LINES="${2:?--lines needs a value}"; shift 2 ;;
    --since)   SINCE="${2:?--since needs a value}"; shift 2 ;;
    --filter)  FILTER="${2:?--filter needs a value}"; shift 2 ;;
    --*)       die "unknown flag: $1" ;;
    *)         PATTERN="$1"; shift ;;
  esac
done

case "$ROLE" in gui|daemon|cli|all) ;; *) die "--role must be gui|daemon|cli|all (got: $ROLE)";; esac

# ---- platform / path resolution --------------------------------------------
app_dir() {
  if [ "$PROFILE" = "default" ] || [ -z "$PROFILE" ]; then
    printf '%s' "$APP_DIR_NAME"
  else
    printf '%s-%s' "$APP_DIR_NAME" "$PROFILE"
  fi
}

# Resolve the logs directory, matching uc_app_paths::app_log_dir().
log_dir() {
  if [ -n "${UC_LOG_DIR:-}" ]; then
    printf '%s' "$UC_LOG_DIR"
    return
  fi
  local app; app="$(app_dir)"
  case "$(uname -s)" in
    Darwin)
      printf '%s/Library/Logs/%s' "$HOME" "$app" ;;
    Linux)
      # XDG state dir, fall back to data-local — same order as the Rust side.
      local base="${XDG_STATE_HOME:-$HOME/.local/state}"
      if [ ! -d "$base" ]; then base="${XDG_DATA_HOME:-$HOME/.local/share}"; fi
      printf '%s/%s/logs' "$base" "$app" ;;
    *)
      # Windows via git-bash/MSYS; LOCALAPPDATA is set in that environment.
      if [ -n "${LOCALAPPDATA:-}" ]; then
        printf '%s/%s/logs' "$LOCALAPPDATA" "$app"
      else
        die "cannot resolve LOCALAPPDATA on this platform; set UC_LOG_DIR explicitly"
      fi ;;
  esac
}

role_stem() {
  case "$1" in
    gui)    printf 'uniclipboard-gui' ;;
    daemon) printf 'uniclipboard-daemon' ;;
    cli)    printf 'uniclipboard-cli' ;;
  esac
}

roles() {
  if [ "$ROLE" = "all" ]; then printf 'gui daemon cli'; else printf '%s' "$ROLE"; fi
}

# Latest file for a role = highest UTC date in the filename (lexical sort).
# `|| true` keeps a no-match (ls fails under pipefail) from tripping `set -e`
# at the `f="$(latest_file ...)"` call sites — a missing role is normal.
latest_file() {
  local dir="$1" role="$2" stem; stem="$(role_stem "$role")"
  { ls -1 "$dir/$stem.json."* 2>/dev/null | sort | tail -1; } || true
}

mtime_epoch() {
  case "$(uname -s)" in
    Darwin) stat -f %m "$1" 2>/dev/null ;;
    *)      stat -c %Y "$1" 2>/dev/null ;;
  esac
}

freshness() {
  local f="$1" now age m
  m="$(mtime_epoch "$f")" || { echo "unknown"; return; }
  [ -n "$m" ] || { echo "unknown"; return; }
  now="$(date +%s)"; age=$(( now - m ))
  if   [ "$age" -lt 120 ];  then echo "live (<2m)"
  elif [ "$age" -lt 600 ];  then echo "recent (<10m)"
  elif [ "$age" -lt 3600 ]; then echo "stale (<1h)"
  elif [ "$age" -lt 86400 ]; then echo "old (<1d)"
  else echo "cold (>1d)"; fi
}

# ---- commands ---------------------------------------------------------------
cmd_paths() {
  local dir; dir="$(log_dir)"
  echo "profile : $PROFILE"
  echo "log dir : $dir"
  [ -d "$dir" ] || { echo "(directory does not exist)"; return; }
  local r f
  for r in $(roles); do
    f="$(latest_file "$dir" "$r")"
    if [ -n "$f" ]; then echo "$r : $f"; else echo "$r : (no file)"; fi
  done
}

cmd_status() {
  local dir; dir="$(log_dir)"
  echo "profile : $PROFILE"
  echo "log dir : $dir"
  if [ ! -d "$dir" ]; then
    echo "(directory does not exist — wrong profile, or that role/app never ran here)"
    echo "hint: try --profile default, or list \$([ -d \"$(dirname "$dir")\" ] && echo siblings)"
    return
  fi
  local r f
  for r in gui daemon cli; do
    f="$(latest_file "$dir" "$r")"
    if [ -n "$f" ]; then
      printf '%-7s %-14s %s\n' "$r" "$(freshness "$f")" "$(basename "$f")"
    else
      printf '%-7s %-14s %s\n' "$r" "-" "(no file)"
    fi
  done
}

# Collect "latest file per selected role" into an array; warn on missing.
collect_files() {
  local dir; dir="$(log_dir)"
  [ -d "$dir" ] || die "log dir does not exist: $dir (run \`status\`; check --profile)"
  local r f any=0
  FILES=()
  for r in $(roles); do
    f="$(latest_file "$dir" "$r")"
    if [ -n "$f" ]; then FILES+=("$f"); any=1
    else echo "uc-logs: no file for role '$r' in $dir" >&2; fi
  done
  [ "$any" = 1 ] || die "no log files found for role(s): $(roles)"
}

cmd_tail() {
  collect_files
  local n="${LINES:-50}" f
  for f in "${FILES[@]}"; do
    echo "==> $(basename "$f") <=="
    tail -n "$n" "$f"
  done
}

cmd_grep() {
  [ -n "$PATTERN" ] || die "grep needs a pattern: uc-logs.sh grep <pattern>"
  collect_files
  local n="${LINES:-50}" f
  for f in "${FILES[@]}"; do
    # Cheap string match first; cap output per file. `|| true`: no-match (grep
    # exit 1) is a normal result, not a failure that should abort under set -e.
    { grep -F -- "$PATTERN" "$f" 2>/dev/null | tail -n "$n" || true; } | while IFS= read -r line; do
      printf '%s\t%s\n' "$(basename "$f")" "$line"
    done
  done
}

cmd_query() {
  need jq
  [ -n "$FILTER" ] || die "query needs --filter '<jq>'"
  collect_files
  local n="${LINES:-50}" f
  for f in "${FILES[@]}"; do
    # `?` swallows per-line errors inside jq; `|| true` covers a non-zero jq
    # exit (e.g. a malformed line) so set -e doesn't abort the whole run.
    { tail -n 5000 "$f" | jq -c "select($FILTER)?" 2>/dev/null | tail -n "$n" || true; }
  done
}

cmd_merge() {
  need jq
  collect_files
  local per="${LINES:-2000}" f role
  # Per-file tail bounds the work; inject .role line-by-line (so one malformed
  # line is skipped, not fatal); then `sort` the whole stream. `timestamp` is
  # always the first field of every line (FlatJsonFormat), so a plain lexical
  # sort of UTC ISO-8601 lines is chronological. Finally drop pre-since lines.
  {
    for f in "${FILES[@]}"; do
      role="$(basename "$f" | sed -E 's/^uniclipboard-([a-z]+)\.json\..*/\1/')"
      tail -n "$per" "$f" | jq -c --arg role "$role" '. + {role: $role}' 2>/dev/null || true
    done
  } | sort \
    | { if [ -n "$SINCE" ]; then jq -c --arg s "$SINCE" 'select(.timestamp >= $s)' 2>/dev/null; else cat; fi; }
}

case "$CMD" in
  status) cmd_status ;;
  paths)  cmd_paths ;;
  tail)   cmd_tail ;;
  grep)   cmd_grep ;;
  query)  cmd_query ;;
  merge)  cmd_merge ;;
  ""|-h|--help|help)
    sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
    ;;
  *) die "unknown command: $CMD (try: status paths tail grep query merge)" ;;
esac
