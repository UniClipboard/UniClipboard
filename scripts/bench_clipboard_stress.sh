#!/usr/bin/env bash
#
# Clipboard stress test — hammer uniclipd with clipboard writes
# and collect per-second CPU / RSS snapshots + leak trend analysis.
#
# Payload mix:
#   - ~2 MB PNG images (random pixels, not compressible)
#   - Large text   (10 KB – 50 KB)
#   - Small text   (50 – 500 chars)
#
# Usage:
#   ./scripts/bench_clipboard_stress.sh [TOTAL] [IMG% LARGE_TEXT% SMALL_TEXT%]
#
# Examples:
#   ./scripts/bench_clipboard_stress.sh              # 10000 iters, 40/40/20
#   ./scripts/bench_clipboard_stress.sh 5000          # 5000  iters, 40/40/20
#   ./scripts/bench_clipboard_stress.sh 10000 30 50 20  # custom ratios
#
# Output:
#   results/bench_clipboard_YYYYMMDD_HHMMSS/
#     ├── resource_usage.csv   — per-second pid,ts,cpu%,rss_kb
#     ├── summary.txt          — stats + leak analysis + ASCII chart
#     └── bench.log            — full run log

set -euo pipefail

TOTAL=${1:-10000}
PCT_IMG=${2:-40}
PCT_LARGE_TEXT=${3:-40}
PCT_SMALL_TEXT=${4:-20}

SUM=$(( PCT_IMG + PCT_LARGE_TEXT + PCT_SMALL_TEXT ))
if (( SUM != 100 )); then
  echo "ERROR: ratios must sum to 100 (got $SUM)" >&2
  exit 1
fi

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
OUT_DIR="results/bench_clipboard_${TIMESTAMP}"
mkdir -p "$OUT_DIR"

CSV="$OUT_DIR/resource_usage.csv"
LOG="$OUT_DIR/bench.log"
SUMMARY="$OUT_DIR/summary.txt"

# ---------- locate daemon PID ----------
DAEMON_PID=$(pgrep -xf '.*uniclipd.*' 2>/dev/null | head -1 || true)
if [[ -z "$DAEMON_PID" ]]; then
  DAEMON_PID=$(pgrep -f '/uniclipd( |$)' 2>/dev/null | head -1 || true)
fi
if [[ -z "$DAEMON_PID" ]]; then
  echo "ERROR: uniclipd is not running. Start the daemon first." >&2
  exit 1
fi
echo "Found uniclipd PID: $DAEMON_PID" | tee "$LOG"

# ---------- generate ~2 MB test PNG (random pixels, resists compression) ----------
TEST_IMG=$(mktemp /tmp/bench_clip_XXXXXX.png)
python3 -c "
import struct, zlib, os
W, H = 1024, 680  # ~2 MB of random RGB data
raw = b''
for _ in range(H):
    raw += b'\x00' + os.urandom(W * 3)  # filter-none + random RGB pixels
def chunk(ctype, data):
    c = ctype + data
    return struct.pack('>I', len(data)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)
with open('$TEST_IMG', 'wb') as f:
    f.write(b'\x89PNG\r\n\x1a\n')
    f.write(chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)))
    f.write(chunk(b'IDAT', zlib.compress(raw, 1)))  # level 1 = fast, stays large
    f.write(chunk(b'IEND', b''))
"
IMG_SIZE=$(wc -c < "$TEST_IMG" | tr -d ' ')
echo "Test image: $TEST_IMG (${IMG_SIZE} bytes, ~$(( IMG_SIZE / 1024 / 1024 )) MB)" | tee -a "$LOG"

# ---------- pre-generate large text payloads ----------
LARGE_TEXT_DIR=$(mktemp -d /tmp/bench_large_text_XXXXXX)
for sz in 10240 20480 30720 40960 51200; do
  LC_ALL=C tr -dc 'A-Za-z0-9 \n,.!?;:()' < /dev/urandom | head -c "$sz" > "$LARGE_TEXT_DIR/$sz.txt" 2>/dev/null || true
done
echo "Large text payloads: 10-50 KB in $LARGE_TEXT_DIR" | tee -a "$LOG"

# ---------- resource monitor (background) ----------
echo "pid,elapsed_s,cpu_pct,rss_kb" > "$CSV"

monitor_resources() {
  local start_ts=$1
  while true; do
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
      echo "WARN: daemon exited during benchmark" >> "$LOG"
      break
    fi
    read -r pcpu rss < <(ps -p "$DAEMON_PID" -o pcpu=,rss= 2>/dev/null || echo "")
    if [[ -n "$pcpu" ]]; then
      local now
      now=$(date +%s)
      echo "$((now - start_ts)),${pcpu},${rss}" >> "$CSV"
    fi
    sleep 1
  done
}

# ---------- baseline snapshot ----------
sleep 2
BASELINE_RSS=$(ps -p "$DAEMON_PID" -o rss= 2>/dev/null | tr -d ' ')
echo "Baseline RSS: ${BASELINE_RSS} KB ($(( BASELINE_RSS / 1024 )) MB)" | tee -a "$LOG"

START_TS=$(date +%s)
monitor_resources "$START_TS" &
MONITOR_PID=$!
trap 'kill $MONITOR_PID 2>/dev/null; rm -f "$TEST_IMG"; rm -rf "$LARGE_TEXT_DIR"' EXIT

# ---------- stress loop ----------
THRESH_IMG=$PCT_IMG
THRESH_LARGE=$(( PCT_IMG + PCT_LARGE_TEXT ))

echo "" | tee -a "$LOG"
echo "Starting stress test: $TOTAL iterations" | tee -a "$LOG"
echo "  mix: ${PCT_IMG}% image(~2MB) / ${PCT_LARGE_TEXT}% large-text(10-50KB) / ${PCT_SMALL_TEXT}% small-text(50-500B)" | tee -a "$LOG"
echo "Start time: $(date)" | tee -a "$LOG"

SUCCESS=0
FAIL=0
IMG_COUNT=0
LARGE_TEXT_COUNT=0
SMALL_TEXT_COUNT=0

LARGE_SIZES=(10240 20480 30720 40960 51200)

for i in $(seq 1 "$TOTAL"); do
  ROLL=$((RANDOM % 100))

  if (( ROLL < THRESH_IMG )); then
    # --- ~2 MB image → system clipboard ---
    if osascript -e "set the clipboard to (read (POSIX file \"$TEST_IMG\") as «class PNGf»)" 2>/dev/null; then
      ((SUCCESS++))
    else
      ((FAIL++))
    fi
    ((IMG_COUNT++))

  elif (( ROLL < THRESH_LARGE )); then
    # --- large text (10-50 KB) → system clipboard via pbcopy ---
    SZ=${LARGE_SIZES[$((RANDOM % 5))]}
    if pbcopy < "$LARGE_TEXT_DIR/$SZ.txt" 2>/dev/null; then
      ((SUCCESS++))
    else
      ((FAIL++))
    fi
    ((LARGE_TEXT_COUNT++))

  else
    # --- small text (50-500 chars) → system clipboard via pbcopy ---
    LEN=$(( RANDOM % 451 + 50 ))
    if LC_ALL=C tr -dc 'A-Za-z0-9 _' < /dev/urandom 2>/dev/null | head -c "$LEN" | pbcopy 2>/dev/null; then
      ((SUCCESS++))
    else
      ((FAIL++))
    fi
    ((SMALL_TEXT_COUNT++))
  fi

  # progress every 500 iterations
  if (( i % 500 == 0 )); then
    ELAPSED=$(( $(date +%s) - START_TS ))
    RATE=$(( i / (ELAPSED > 0 ? ELAPSED : 1) ))
    CUR_RSS=$(ps -p "$DAEMON_PID" -o rss= 2>/dev/null | tr -d ' ')
    echo "  [$i/$TOTAL] ${ELAPSED}s ${RATE}/s rss=${CUR_RSS}KB($(( CUR_RSS / 1024 ))MB) ok=$SUCCESS fail=$FAIL (img=$IMG_COUNT lg=$LARGE_TEXT_COUNT sm=$SMALL_TEXT_COUNT)" | tee -a "$LOG"
  fi
done

END_TS=$(date +%s)
DURATION=$(( END_TS - START_TS ))

# ---------- cooldown: sample 30s after load stops ----------
echo "" | tee -a "$LOG"
echo "Cooldown: sampling RSS for 30s after load stops..." | tee -a "$LOG"
sleep 30
FINAL_RSS=$(ps -p "$DAEMON_PID" -o rss= 2>/dev/null | tr -d ' ')

# ---------- stop monitor ----------
kill $MONITOR_PID 2>/dev/null || true
wait $MONITOR_PID 2>/dev/null || true

# ---------- compute summary + trend + chart ----------
{
  echo "=== Stress Test Complete ==="
  echo "Duration: ${DURATION}s (+30s cooldown)"
  echo "Total: $TOTAL (image: $IMG_COUNT, large-text: $LARGE_TEXT_COUNT, small-text: $SMALL_TEXT_COUNT)"
  echo "Success: $SUCCESS  Failed: $FAIL"
  echo "Avg rate: $(( TOTAL / (DURATION > 0 ? DURATION : 1) )) ops/s"
  echo ""
  echo "Baseline RSS: ${BASELINE_RSS} KB ($(( BASELINE_RSS / 1024 )) MB)"
  echo "Final RSS:    ${FINAL_RSS} KB ($(( FINAL_RSS / 1024 )) MB)"
  echo "RSS delta:    $(( FINAL_RSS - BASELINE_RSS )) KB ($(( (FINAL_RSS - BASELINE_RSS) / 1024 )) MB)"
} | tee -a "$LOG"

# awk: summary stats + leak trend (first/last 10% avg) + ASCII sparkline
awk -F, '
NR == 1 { next }
{
  cpu = $2 + 0; rss = $3 + 0
  n++
  sum_cpu += cpu; sum_rss += rss
  if (n == 1 || cpu < min_cpu) min_cpu = cpu
  if (n == 1 || cpu > max_cpu) max_cpu = cpu
  if (n == 1 || rss < min_rss) min_rss = rss
  if (n == 1 || rss > max_rss) max_rss = rss
  all_rss[n] = rss
  all_cpu[n] = cpu
  elapsed[n] = $1 + 0
}
END {
  if (n == 0) { print "No data collected"; exit }

  printf "\n=== Resource Usage Summary ===\n"
  printf "Samples: %d (1/sec)\n\n", n

  printf "CPU %%:\n"
  printf "  min=%.1f  max=%.1f  avg=%.1f\n\n", min_cpu, max_cpu, sum_cpu/n

  printf "RSS:\n"
  printf "  min=%d KB (%d MB)  max=%d KB (%d MB)  avg=%d KB (%d MB)\n\n", \
    min_rss, min_rss/1024, max_rss, max_rss/1024, sum_rss/n, (sum_rss/n)/1024

  # --- leak trend: compare first 10% vs last 10% ---
  seg = int(n * 0.1)
  if (seg < 1) seg = 1
  first_sum = 0; last_sum = 0
  for (i = 1; i <= seg; i++) first_sum += all_rss[i]
  for (i = n - seg + 1; i <= n; i++) last_sum += all_rss[i]
  first_avg = first_sum / seg
  last_avg = last_sum / seg
  delta = last_avg - first_avg
  pct = (first_avg > 0) ? (delta / first_avg * 100) : 0

  printf "=== Leak Trend Analysis ===\n"
  printf "First 10%% avg RSS: %d KB (%d MB)\n", first_avg, first_avg/1024
  printf "Last  10%% avg RSS: %d KB (%d MB)\n", last_avg, last_avg/1024
  printf "Delta:              %+d KB (%+d MB, %+.1f%%)\n", delta, delta/1024, pct
  if (delta > 0 && pct > 20)
    printf "Verdict: ⚠️  RSS grew >20%% — possible memory leak\n\n"
  else if (delta > 0 && pct > 5)
    printf "Verdict: 🔶 RSS grew %.0f%% — moderate, may be caching\n\n", pct
  else
    printf "Verdict: ✅ RSS stable (delta %.0f%%)\n\n", pct

  # --- ASCII sparkline chart (60 columns wide) ---
  printf "=== RSS Timeline (MB) ===\n"
  COLS = 60
  ROWS = 20
  # bucket samples into COLS columns
  bucket_size = (n > COLS) ? int(n / COLS) : 1
  cols_actual = int(n / bucket_size)
  if (cols_actual < 1) cols_actual = 1
  if (cols_actual > COLS) cols_actual = COLS

  for (c = 1; c <= cols_actual; c++) {
    s = 0; cnt = 0
    for (j = (c-1)*bucket_size+1; j <= c*bucket_size && j <= n; j++) {
      s += all_rss[j]; cnt++
    }
    bar_val[c] = (cnt > 0) ? s/cnt : 0
    bar_time[c] = elapsed[(c-1)*bucket_size+1]
  }

  # find chart range
  chart_min = bar_val[1]; chart_max = bar_val[1]
  for (c = 1; c <= cols_actual; c++) {
    if (bar_val[c] < chart_min) chart_min = bar_val[c]
    if (bar_val[c] > chart_max) chart_max = bar_val[c]
  }
  # add 10% padding
  chart_range = chart_max - chart_min
  if (chart_range < 1024) chart_range = 1024  # min 1 MB range
  chart_min_display = chart_min - chart_range * 0.05
  if (chart_min_display < 0) chart_min_display = 0
  chart_max_display = chart_max + chart_range * 0.05
  chart_range_display = chart_max_display - chart_min_display

  # render top-down
  for (row = ROWS; row >= 1; row--) {
    threshold = chart_min_display + chart_range_display * row / ROWS
    if (row == ROWS)
      printf "%6d MB │", int(chart_max_display/1024)
    else if (row == 1)
      printf "%6d MB │", int(chart_min_display/1024)
    else if (row == int(ROWS/2))
      printf "%6d MB │", int((chart_min_display + chart_range_display/2)/1024)
    else
      printf "         │"

    for (c = 1; c <= cols_actual; c++) {
      level = (bar_val[c] - chart_min_display) / chart_range_display * ROWS
      if (level >= row - 0.5)
        printf "█"
      else if (level >= row - 1)
        printf "▄"
      else
        printf " "
    }
    printf "\n"
  }
  # x-axis
  printf "         └"
  for (c = 1; c <= cols_actual; c++) printf "─"
  printf "\n"
  printf "          0s"
  # right-align last timestamp
  gap = cols_actual - 2 - length(sprintf("%ds", bar_time[cols_actual]))
  for (c = 1; c <= gap; c++) printf " "
  printf "%ds\n", bar_time[cols_actual]
}
' "$CSV" | tee "$SUMMARY" | tee -a "$LOG"

echo "" | tee -a "$LOG"
echo "Results saved to: $OUT_DIR/" | tee -a "$LOG"
echo "  resource_usage.csv — per-second snapshots" | tee -a "$LOG"
echo "  summary.txt        — stats + leak analysis + chart" | tee -a "$LOG"
echo "  bench.log          — full run log" | tee -a "$LOG"
