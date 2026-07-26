#!/usr/bin/env bash
# ios-logs.sh — drive the UniClipboard iOS app in a simulator and read its
# OSLog, so the agent can diagnose mobile-sync / reducer bugs without asking the
# user to paste logs. See SKILL.md for the two-channel model and gotchas.
#
# Subcommands:
#   drive  [SERVER_URL]            boot a sim, install newest build, inject the
#                                  Rust-core flag ON + an active server, relaunch.
#                                  SERVER_URL default = http://127.0.0.1:59999
#                                  (a dead URL still makes the engine tick).
#   stream [SECONDS] [CATEGORY]    LIVE debug stream (per-tick decisions). Default
#                                  15s, all categories. debug is NOT persisted, so
#                                  this is the ONLY way to see preamble/route/skip.
#   show   [DURATION] [CATEGORY]   PERSISTED notice/error trail, queryable after
#                                  the fact. Default last 5m, all categories.
#
# CATEGORY (optional): sync | network | store | app | intents. Omit for all.
set -uo pipefail

SUBSYSTEM="app.uniclipboard"
BUNDLE="app.uniclipboard.UniClipboard"
APPGROUP="group.app.uniclipboard.UniClipboard"

die() { echo "ERROR: $*" >&2; exit 1; }

booted_device() {
  local d
  d=$(xcrun simctl list devices booted 2>/dev/null | grep -oE '\([0-9A-F-]{36}\)' | head -1 | tr -d '()')
  if [ -z "$d" ]; then
    d=$(xcrun simctl list devices available 2>/dev/null | grep -E 'iPhone' | head -1 | grep -oE '\([0-9A-F-]{36}\)' | tr -d '()')
    [ -n "$d" ] || die "no available iPhone simulator"
    echo "booting $d ..." >&2
    xcrun simctl boot "$d" >/dev/null 2>&1
    xcrun simctl bootstatus "$d" -b >/dev/null 2>&1
  fi
  echo "$d"
}

newest_app() {
  # Most recently built simulator .app product.
  ls -dt "$HOME"/Library/Developer/Xcode/DerivedData/UniClipboard-*/Build/Products/Debug-iphonesimulator/UniClipboard.app 2>/dev/null | head -1
}

predicate() {
  local cat="${1:-}"
  if [ -n "$cat" ]; then
    echo "subsystem == \"$SUBSYSTEM\" AND category == \"$cat\""
  else
    echo "subsystem == \"$SUBSYSTEM\""
  fi
}

cmd_drive() {
  local url="${1:-http://127.0.0.1:59999}"
  local dev app
  dev=$(booted_device); echo "device: $dev"
  app=$(newest_app); [ -n "$app" ] || die "no built UniClipboard.app — run xcodebuild first"
  echo "app: $app"
  xcrun simctl install "$dev" "$app" || die "install failed"
  xcrun simctl terminate "$dev" "$BUNDLE" >/dev/null 2>&1
  # Flag ON (harmless no-op once the native paths are deleted and Rust is the
  # only path — the key just goes unread).
  xcrun simctl spawn "$dev" defaults write "$APPGROUP" mobileCore.syncClientUsesRustCore -bool YES
  # Active server (the engine only ticks with one). Codable ServerConfigList blob.
  local json hex
  json="{\"configs\":[{\"id\":\"sim-test\",\"name\":\"SimTest\",\"username\":\"u\",\"password\":\"p\",\"url\":\"$url\",\"urls\":[\"$url\"]}],\"activeConfigId\":\"sim-test\"}"
  hex=$(printf '%s' "$json" | xxd -p | tr -d '\n')
  xcrun simctl spawn "$dev" defaults write "$APPGROUP" server_config_list -data "$hex"
  echo "injected: flag=ON server=$url"
  xcrun simctl launch "$dev" "$BUNDLE"
}

cmd_stream() {
  local secs="${1:-15}" cat="${2:-}" dev pred
  dev=$(booted_device)
  pred=$(predicate "$cat")
  echo "=== LIVE debug stream ${secs}s · $pred ===" >&2
  # macOS has no `timeout`; perl fork+alarm bounds the otherwise-endless stream.
  SECS="$secs" perl -e 'my $p=fork; if(!$p){exec @ARGV; exit 1} $SIG{ALRM}=sub{kill "TERM",$p}; alarm $ENV{SECS}; waitpid($p,0)' \
    xcrun simctl spawn "$dev" log stream --level debug --predicate "$pred" --style compact 2>&1 \
    | grep -avE '^Filtering|getpwuid'
}

cmd_show() {
  local dur="${1:-5m}" cat="${2:-}" dev pred
  dev=$(booted_device)
  pred=$(predicate "$cat")
  echo "=== PERSISTED notice/error · last $dur · $pred ===" >&2
  xcrun simctl spawn "$dev" log show --last "$dur" --predicate "$pred" --style compact 2>&1 \
    | grep -avE '^Filtering|^Timestamp +Th|getpwuid'
}

case "${1:-}" in
  drive)  shift; cmd_drive "$@" ;;
  stream) shift; cmd_stream "$@" ;;
  show)   shift; cmd_show "$@" ;;
  *) echo "usage: ios-logs.sh {drive [URL] | stream [SECS] [CAT] | show [DUR] [CAT]}" >&2; exit 2 ;;
esac
