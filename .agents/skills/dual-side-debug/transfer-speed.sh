#!/usr/bin/env bash
# transfer-speed.sh — Reconstruct the per-checkpoint download speed curve from
# Windows-side "blob fetch: progress checkpoint" log entries.
#
# Each checkpoint carries (timestamp, bytes, elapsed_ms, flow.id, conn, ...),
# so we can derive both instantaneous and cumulative-average MB/s without any
# special instrumentation.
#
# Usage:
#   transfer-speed.sh [--profile <name>] [--side mac|win] [--flow <flow.id>]
#                     [--since <ISO8601>] [--until <ISO8601>] [--every N]
#                     [--show-conn]
#
# Defaults: --side win  --every 1
#
# Examples:
#   # Full curve, every checkpoint, current dev profile, Windows side:
#   transfer-speed.sh
#
#   # Sample every 5th checkpoint within a time window:
#   transfer-speed.sh --since 2026-05-24T01:27:00Z --until 2026-05-24T01:30:30Z --every 5
#
#   # Restrict to one specific transfer (by flow.id) and print which conn path it used:
#   transfer-speed.sh --flow 019e5798-6c03-7ac2-b484-1fc777ada87e --show-conn

set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DUAL="$HERE/dual-logs.sh"

if [[ ! -x "$DUAL" ]]; then
  echo "transfer-speed.sh: cannot find dual-logs.sh next to me ($DUAL)" >&2
  exit 1
fi

PROFILE=""
SIDE="win"
FLOW=""
SINCE=""
UNTIL=""
EVERY=1
SHOW_CONN=0

while (( $# > 0 )); do
  case "$1" in
    --profile)   PROFILE="$2"; shift 2 ;;
    --side)      SIDE="$2"; shift 2 ;;
    --flow)      FLOW="$2"; shift 2 ;;
    --since)     SINCE="$2"; shift 2 ;;
    --until)     UNTIL="$2"; shift 2 ;;
    --every)     EVERY="$2"; shift 2 ;;
    --show-conn) SHOW_CONN=1; shift ;;
    -h|--help)
      sed -n '2,30p' "$0"
      exit 0 ;;
    *)
      echo "transfer-speed.sh: unknown arg: $1" >&2
      exit 2 ;;
  esac
done

if [[ "$SIDE" != "win" && "$SIDE" != "mac" ]]; then
  echo "transfer-speed.sh: --side must be 'win' or 'mac' (got: $SIDE)" >&2
  exit 2
fi

# Build the jq filter incrementally so empty options don't bake in dead conditions.
JQ_FILTER='. | select(.message == "blob fetch: progress checkpoint")'
[[ -n "$FLOW"  ]] && JQ_FILTER="$JQ_FILTER"' | select(."flow.id" == "'"$FLOW"'")'
[[ -n "$SINCE" ]] && JQ_FILTER="$JQ_FILTER"' | select(.timestamp >= "'"$SINCE"'")'
[[ -n "$UNTIL" ]] && JQ_FILTER="$JQ_FILTER"' | select(.timestamp <= "'"$UNTIL"'")'
JQ_FILTER="$JQ_FILTER"' | "\(.timestamp)\t\(.bytes)\t\(.elapsed_ms)\t\(."flow.id" // "-")\t\(.conn // "-")"'

DUAL_ARGS=(query --side "$SIDE" --filter "$JQ_FILTER")
[[ -n "$PROFILE" ]] && DUAL_ARGS=(--profile "$PROFILE" "${DUAL_ARGS[@]}")

# Strip the "===== [side] ... =====" banners that dual-logs.sh prints and any
# accidental jq-quoted output, then run the speed math.
"$DUAL" "${DUAL_ARGS[@]}" \
  | grep -v '^=====' \
  | grep -v '^$' \
  | sed 's/^"//;s/"$//;s/\\t/\t/g' \
  | awk -F'\t' -v every="$EVERY" -v show_conn="$SHOW_CONN" '
    BEGIN {
      n = 0;
      prev_b = -1; prev_ms = -1;
      first_b = -1; first_ms = -1;
      printf "  %-26s  %9s  %10s  %10s%s\n", "timestamp", "MB_total", "inst_MBps", "avg_MBps", (show_conn ? "  conn" : "");
    }
    NF >= 3 {
      n++;
      ts = $1; b = $2 + 0; ms = $3 + 0; flow = ($4 == "" ? "-" : $4); conn = ($5 == "" ? "-" : $5);
      if (first_ms < 0) { first_b = b; first_ms = ms; }
      inst = 0;
      if (prev_ms >= 0 && ms > prev_ms) {
        inst = (b - prev_b) / ((ms - prev_ms) / 1000.0) / 1048576.0;
      }
      span_ms = ms - first_ms;
      avg = (span_ms > 0) ? ((b - first_b) / (span_ms / 1000.0) / 1048576.0) : 0;
      if ((n - 1) % every == 0) {
        if (show_conn) {
          printf "  %-26s  %9.1f  %10.2f  %10.2f  %s\n", ts, b/1048576.0, inst, avg, conn;
        } else {
          printf "  %-26s  %9.1f  %10.2f  %10.2f\n", ts, b/1048576.0, inst, avg;
        }
      }
      last_ts = ts; last_b = b; last_ms = ms;
      prev_b = b; prev_ms = ms;
    }
    END {
      if (n == 0) {
        print "  (no progress checkpoints matched)" > "/dev/stderr";
        exit 1;
      }
      span_ms = last_ms - first_ms;
      total_mb = (last_b - first_b) / 1048576.0;
      overall = (span_ms > 0) ? (total_mb / (span_ms / 1000.0)) : 0;
      printf "\n  summary: %d checkpoints, %.1f MB over %.1f s -> %.2f MB/s overall\n",
             n, total_mb, span_ms/1000.0, overall;
    }
  '
