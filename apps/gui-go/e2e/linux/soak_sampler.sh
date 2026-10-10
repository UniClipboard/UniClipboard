#!/usr/bin/env bash
# Soak sampler for the Linux host (runs INSIDE the graphical session, next to the app under test).
#   soak_sampler.sh <run-dir> <app-copy> <seconds> [csv-name]      (TOGGLE=0 samples only, no panel toggles)
# Every 60 s it records, from /proc, RSS (kB), PSS (kB, from smaps_rollup), thread and fd counts of the GUI process of <app-copy> and of
# EVERY descendant (daemon, WebKitWebProcess, WebKitNetworkProcess, WebKitGPUProcess, helpers). Descendants are found by parent pid, not by
# name: the kernel truncates comm to 15 characters ("WebKitWebProces"), so name matching misses them. RSS double-counts shared pages; PSS
# is the apportioned figure and the tree total is the sum of PSS, never of RSS.
# When TOGGLE is not 0, every 300 s it toggles the quick panel twice through the real single-instance forwarding (`<app-copy> --quick-panel`),
# so the run is an ACTIVE soak, not an idle one. It writes DIR/<csv-name> (default soak.csv) and DIR/soak-events.log.
set -u
dir=$1; app=$2; secs=$3; csvname=${4:-soak.csv}; toggle=${TOGGLE:-1}
mkdir -p "$dir"; csv=$dir/$csvname; ev=$dir/soak-events.log
echo "epoch,iso,proc,pid,ppid,rss_kb,pss_kb,threads,fds" > "$csv"
start=$(date +%s); last_toggle=0
gui_pid() { for p in $(pgrep -x uniclipboard 2>/dev/null); do case "$(readlink /proc/$p/exe 2>/dev/null)" in /tmp/appimage_extracted_*|/tmp/.mount_*) echo "$p"; return;; esac; done; }
descendants() { local p=$1 c; echo "$p"; for c in $(pgrep -P "$p" 2>/dev/null); do descendants "$c"; done; }
while :; do
  now=$(date +%s); el=$((now-start)); [ "$el" -ge "$secs" ] && break
  root=$(gui_pid)
  if [ -n "${root:-}" ]; then
    for pid in $(descendants "$root"); do
      [ -r /proc/$pid/status ] || continue
      name=$(awk '/^Name:/{print $2}' /proc/$pid/status); ppid=$(awk '/^PPid:/{print $2}' /proc/$pid/status)
      rss=$(awk '/VmRSS/{print $2}' /proc/$pid/status); pss=$(awk '/^Pss:/{print $2}' /proc/$pid/smaps_rollup 2>/dev/null)
      th=$(awk '/Threads/{print $2}' /proc/$pid/status); fd=$(ls /proc/$pid/fd 2>/dev/null | wc -l)
      echo "$now,$(date -Is),$name,$pid,$ppid,${rss:-},${pss:-},$th,$fd" >> "$csv"
    done
  else
    echo "$now no-gui-process" >> "$ev"
  fi
  if [ "$toggle" != 0 ] && [ $((now-last_toggle)) -ge 300 ]; then
    last_toggle=$now
    for n in 1 2; do
      timeout 30 env -u UC_DISABLE_SYSTEM_CLIPBOARD APPIMAGE_EXTRACT_AND_RUN=1 XDG_SESSION_TYPE=wayland "$app" --quick-panel >/dev/null 2>&1; echo "$now toggle-$n rc=$?" >> "$ev"
      sleep 3
    done
  fi
  sleep 60
done
echo "$(date +%s) done after ${el}s ($csvname)" >> "$ev"
