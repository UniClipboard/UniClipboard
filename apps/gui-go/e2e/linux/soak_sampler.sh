#!/usr/bin/env bash
# Soak sampler for the Linux host (runs INSIDE the graphical session, next to the app under test).
#   soak_sampler.sh <run-dir> <app-copy> <seconds>
# Every 60 s it records, from /proc, the RSS (kB), thread and fd counts of the GUI, the daemon and the WebKit processes that belong
# to <app-copy>'s mount, and every 300 s it toggles the quick panel twice through the real single-instance forwarding
# (`<app-copy> --quick-panel`) so the run is an ACTIVE soak, not an idle one. It writes DIR/soak.csv and DIR/soak-events.log.
set -u
dir=$1; app=$2; secs=$3
mkdir -p "$dir"; csv=$dir/soak.csv; ev=$dir/soak-events.log
echo "epoch,iso,proc,pid,rss_kb,threads,fds" > "$csv"
start=$(date +%s); last_toggle=0
env_args=(UC_DISABLE_SYSTEM_CLIPBOARD= APPIMAGE_EXTRACT_AND_RUN=1 XDG_SESSION_TYPE=wayland)
while :; do
  now=$(date +%s); el=$((now-start)); [ "$el" -ge "$secs" ] && break
  for name in uniclipboard uniclipd WebKitWebProcess WebKitNetworkProcess WebKitGPUProcess; do
    for pid in $(pgrep -x "$name" 2>/dev/null); do
      [ -r /proc/$pid/status ] || continue
      exe=$(readlink /proc/$pid/exe 2>/dev/null || true)
      case "$exe" in /tmp/appimage_extracted_*|/tmp/.mount_*) ;; *) continue;; esac
      rss=$(awk '/VmRSS/{print $2}' /proc/$pid/status); th=$(awk '/Threads/{print $2}' /proc/$pid/status); fd=$(ls /proc/$pid/fd 2>/dev/null | wc -l)
      echo "$now,$(date -Is),$name,$pid,$rss,$th,$fd" >> "$csv"
    done
  done
  if [ $((now-last_toggle)) -ge 300 ]; then
    last_toggle=$now
    env -u UC_DISABLE_SYSTEM_CLIPBOARD APPIMAGE_EXTRACT_AND_RUN=1 XDG_SESSION_TYPE=wayland "$app" --quick-panel >/dev/null 2>&1; echo "$now toggle-1 rc=$?" >> "$ev"
    sleep 3
    env -u UC_DISABLE_SYSTEM_CLIPBOARD APPIMAGE_EXTRACT_AND_RUN=1 XDG_SESSION_TYPE=wayland "$app" --quick-panel >/dev/null 2>&1; echo "$now toggle-2 rc=$?" >> "$ev"
  fi
  sleep 60
done
echo "$(date +%s) done after ${el}s" >> "$ev"
