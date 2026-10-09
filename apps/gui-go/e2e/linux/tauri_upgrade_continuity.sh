#!/usr/bin/env bash
# Tauri -> Go in-place package upgrade with data written by the released Tauri build itself
# (docs/architecture/gui-go-tauri-upgrade-continuity.md, issue #1900).
#
#   tauri_upgrade_continuity.sh <stopped|running> <image> <new go deb|rpm> <expected daemon sha256> <outdir>
#   tauri_upgrade_continuity.sh <samepath|newpath> <image> <new go AppImage> <expected daemon sha256> <outdir> <old Tauri AppImage>
#
# deb/rpm: the image already has the official Tauri package installed (see the Dockerfile recipe in the document), so the "before" state is
# produced by the released GUI and the released daemon, not by the candidate. AppImage: the image has no Tauri package; the official Tauri
# AppImage is copied to a user directory and run from there. samepath replaces that file with the Go AppImage (what the Tauri updater does
# with a downloaded update); newpath removes it and puts the Go AppImage under its own release name (a manual download). The AppImages run with
# APPIMAGE_EXTRACT_AND_RUN=1, so the FUSE mount is not exercised here. Phases:
#   1. launch the released Tauri GUI, initialize the encrypted profile, enable autostart, capture clipboard history (text and image)
#   2. restart the Tauri GUI so its own startup reconciliation writes the autostart entry; record the "before" state
#   3. stopped: quit Tauri completely. running: leave the Tauri GUI and daemon alive (issue #1836 shape)
#   4. install the Go deb over it with the package manager
#   5. start the Go host exactly as the login manager would (the Exec of the entry the Tauri build wrote), record the "after" state
# Every assertion is a PASS/FAIL line in <outdir>/results.tsv; the exit status is the number of failures.
# Container evidence only: headless Xvfb, an unlocked throwaway gnome-keyring as the Secret Service, no desktop session, no login/logout.
set -uo pipefail
if [ "${1:-}" != "--inner" ]; then
  scenario="${1:?stopped|running}"; image="${2:?image}"; new="${3:?new go deb or rpm}"; kind="${new##*.}"; case "$kind" in deb|rpm|AppImage) ;; *) echo "package must be .deb, .rpm or .AppImage" >&2; exit 2 ;; esac; sha="${4:?daemon sha256}"; out="${5:?outdir}"
  old_mount=(); old_sha=""
  if [ "$kind" = AppImage ]; then
    case "$scenario" in samepath|newpath) ;; *) exit 2 ;; esac
    old="${6:?old Tauri AppImage}"; old_mount=(-v "$old:/in/old.AppImage:ro"); old_sha="$(shasum -a 256 "$old" | cut -d' ' -f1)"
  else
    case "$scenario" in stopped|running) ;; *) exit 2 ;; esac
  fi
  [ ! -e "$out" ] || { echo "output already exists: $out" >&2; exit 2; }
  mkdir -p "$out/source"; out="$(cd "$out" && pwd)"
  here="$(cd "$(dirname "$0")" && pwd)"
  # Freeze the harness before running; a shared checkout may change while the container runs.
  cp "$here/tauri_upgrade_continuity.sh" "$here/tauri_continuity_probe.py" "$here/tauri_continuity_compare.py" "$here/keyring_inventory.py" "$here/localstorage_inventory.py" "$out/source/"
  (cd "$out/source" && shasum -a 256 ./* > "$out/harness.sha256")
  docker image inspect "$image" --format '{{.Id}}' > "$out/image-id.txt"
  shasum -a 256 "$new" ${old:+"$old"} > "$out/package-inputs.sha256"
  docker run --rm --init --cap-add IPC_LOCK --platform "${UC_DOCKER_PLATFORM:-linux/arm64}" -v "$out/source:/w:ro" -v "$new:/in/new.$kind:ro" ${old_mount[@]+"${old_mount[@]}"} -v "$out:/out" \
    "$image" bash /w/tauri_upgrade_continuity.sh --inner "$scenario" "$sha" "$kind" "$old_sha" > "$out/run.log" 2>&1
  rc=$?
  tail -n 60 "$out/results.tsv" 2>/dev/null; exit "$rc"
fi

shift; scenario="$1"; sha="$2"; kind="$3"; old_sha="${4:-}"
results=/out/results.tsv; : > "$results"; failures=0
ok()   { printf 'PASS\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
bad()  { printf 'FAIL\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; failures=$((failures + 1)); }
info() { printf 'INFO\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
check() { local name="$1"; shift; local rc=0; "$@" >/tmp/check.out 2>&1 || rc=$?; { printf "\n%s: " "$name"; printf "%q " "$@"; printf "\n"; cat /tmp/check.out; } >> /out/commands.log; if [ "$rc" = 0 ]; then ok "$name"; else bad "$name" "$(tr '\n' ' ' < /tmp/check.out | cut -c1-300)"; fi; }
{ echo "image: $(. /etc/os-release && echo "$PRETTY_NAME")"; echo "machine: $(uname -m)"; ldd --version | head -1; echo "scenario: $scenario"; } | tee /out/host.txt

export HOME=/root; mkdir -p /root/.local/share /root/.config /root/.cache  # gnome-keyring needs ~/.cache for its sockets
export UC_ROOT=/root/.local/share/app.uniclipboard.desktop

dbus-run-session -- bash -s "$scenario" "$sha" "$kind" "$old_sha" <<'SESSION'
set -u
scenario="$1"; sha="$2"; kind="$3"; old_sha="${4:-}"
results=/out/results.tsv; failures=0
ok()   { printf 'PASS\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
bad()  { printf 'FAIL\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
info() { printf 'INFO\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
check() { local name="$1"; shift; local rc=0; "$@" >/tmp/check.out 2>&1 || rc=$?; { printf "\n%s: " "$name"; printf "%q " "$@"; printf "\n"; cat /tmp/check.out; } >> /out/commands.log; if [ "$rc" = 0 ]; then ok "$name"; else bad "$name" "$(tr '\n' ' ' < /tmp/check.out | cut -c1-300)"; fi; }

printf "uc-throwaway" | gnome-keyring-daemon --foreground --unlock --components=secrets > /tmp/keyring.env 2> /tmp/keyring.err &
for i in $(seq 100); do dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true" && break; sleep 0.1; done
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & xvfb=$!
export DISPLAY=:99; for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
trap 'pkill -x uniclipboard 2>/dev/null; pkill -x uniclipd 2>/dev/null; kill $xvfb 2>/dev/null' EXIT

OLD_BIN=/usr/bin/uniclipboard
if [ "$kind" = AppImage ]; then
  export APPIMAGE_EXTRACT_AND_RUN=1
  APPS=/root/Apps; OLD_BIN=$APPS/UniClipboard_1.1.2_aarch64.AppImage; NEW_BIN=$APPS/UniClipboard_1.1.2_aarch64.AppImage
  [ "$scenario" = newpath ] && NEW_BIN=$APPS/UniClipboard_1.1.3_aarch64.AppImage
  mkdir -p $APPS; install -m 0755 /in/old.AppImage $OLD_BIN
  pm_old_version() { echo 1.1.2; }  # the file is the released v1.1.2 asset; its sha256 is recorded in package-inputs.sha256 and compared with SHA256SUMS.txt
  pm_state() { sha256sum $APPS/*.AppImage; }
  pm_old_present() { [ -e $OLD_BIN ] && [ "$(sha256sum $OLD_BIN | cut -d' ' -f1)" = "$old_sha" ]; }
  pm_owners() { echo "uniclipboard "; }
  pm_install() {
    if [ "$scenario" = samepath ]; then install -m 0755 /in/new.AppImage $APPS/.replace.tmp && mv -f $APPS/.replace.tmp $NEW_BIN
    else rm -f $OLD_BIN && install -m 0755 /in/new.AppImage $NEW_BIN; fi
  }
elif [ "$kind" = deb ]; then
  pm_install() { apt-get install -y /in/new.deb; }
  pm_old_version() { dpkg-query -W -f '${Version}' uni-clipboard 2>/dev/null; }
  pm_state() { dpkg-query -W -f '${Package} ${Version} ${db:Status-Abbrev}\n' uni-clipboard uniclipboard 2>&1; }
  pm_old_present() { dpkg-query -W -f '${db:Status-Abbrev}' uni-clipboard 2>/dev/null | grep -q '^ii'; }
  pm_owners() { dpkg -S "$1" 2>/dev/null | cut -d: -f1 | sort -u | tr '\n' ' '; }
else
  pm_install() { dnf -y install /in/new.rpm; }
  pm_old_version() { rpm -q --qf '%{VERSION}' uni-clipboard 2>/dev/null; }
  pm_state() { rpm -qa --qf '%{NAME} %{VERSION}-%{RELEASE}\n' | grep -E '^uni(-)?clipboard ' ; }
  pm_old_present() { rpm -q uni-clipboard >/dev/null 2>&1; }
  pm_owners() { rpm -qf --qf '%{NAME}\n' "$1" 2>/dev/null | sort -u | tr '\n' ' '; }
fi

stop_all() {
  pkill -TERM -x uniclipboard 2>/dev/null; for i in $(seq 40); do pgrep -x uniclipboard >/dev/null || break; sleep .25; done
  pkill -TERM -x uniclipd 2>/dev/null; for i in $(seq 60); do pgrep -x uniclipd >/dev/null || break; sleep .25; done
  pgrep -x uniclipboard >/dev/null || pgrep -x uniclipd >/dev/null && { pkill -KILL -x uniclipboard; pkill -KILL -x uniclipd; return 1; }; return 0
}
wait_ready() {  # the GUI process, the daemon and a WebView process are all up
  for i in $(seq 120); do pgrep -x uniclipd >/dev/null && pgrep -f "[W]ebKitWebProcess" >/dev/null && return 0; sleep 1; done; return 1
}
datafiles() {  # relative path -> sha256 of every persistent file of the data root; caches, locks and connection files are volatile
  python3 - "$1" <<'PY'
import hashlib, json, os, sys
root = os.environ["UC_ROOT"]; skip_dirs = {"WebKitCache", "CacheStorage", "localstorage", "file-cache", "logs"}
skip_names = {"daemon.conn", "daemon-startup.conn", ".daemon-pid", ".uniclipd.lock", "hsts-storage.sqlite"}
out = {}
for base, dirs, files in os.walk(root):
    dirs[:] = sorted(d for d in dirs if d not in skip_dirs)
    for name in sorted(files):
        if name in skip_names or name.endswith(("-wal", "-shm")) or name.startswith("daemon-run.json"):
            continue
        path = os.path.join(base, name)
        out[os.path.relpath(path, root)] = hashlib.sha256(open(path, "rb").read()).hexdigest()
json.dump(out, open(sys.argv[1], "w"), indent=2, sort_keys=True)
PY
}
png() { python3 -c "
import struct, sys, zlib
w = h = 64
raw = b''.join(b'\\x00' + b''.join(bytes([(x * 4) % 256, (y * 4) % 256, 128, 255]) for x in range(w)) for y in range(h))
def chunk(t, d): c = struct.pack('>I', len(d)) + t + d; return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
sys.stdout.buffer.write(b'\\x89PNG\\r\\n\\x1a\\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))
"; }

# ---- phase 1: the released Tauri build creates the profile ----
check "the installed package is the released Tauri build" test "$(pm_old_version)" = 1.1.2
{ pm_state; [ "$kind" = AppImage ] || sha256sum /usr/bin/uniclipboard /usr/bin/uniclipd; } > /out/tauri-installed.txt
$OLD_BIN > /tmp/tauri-gui-1.log 2>&1 &
check "Tauri GUI, daemon and WebView start" wait_ready
check "initialize the encrypted profile through the Tauri-era daemon" python3 /w/tauri_continuity_probe.py init /out/tauri-init.json
check "enable autostart in the settings" python3 /w/tauri_continuity_probe.py configure /out/tauri-configure.json
png | xclip -selection clipboard -t image/png; sleep 4
printf 'hello-one' | xclip -selection clipboard; sleep 3
printf 'hello-two' | xclip -selection clipboard; sleep 3
stop_all && ok "the Tauri build exits cleanly" || bad "the Tauri build exits cleanly" "processes had to be killed"
cp /tmp/tauri-gui-1.log /out/

# ---- phase 2: second start so the Tauri build reconciles the OS autostart entry itself ----
mkdir -p /out/before /out/after /out/control
pkill -x xclip 2>/dev/null; sleep 1; [ -z "$(xclip -o -selection clipboard 2>/dev/null)" ] && ok "the X11 clipboard is empty before the Tauri login start" || bad "the X11 clipboard is empty before the Tauri login start"
$OLD_BIN --autostart > /tmp/tauri-gui-2.log 2>&1 &
check "Tauri restarts on the profile it created" wait_ready
for i in $(seq 40); do [ -e /root/.config/autostart/UniClipboard.desktop ] && break; sleep .5; done
sleep 5
# Baseline only (not an invariant): v1.1.2 lists the history once, immediately at launch, and loses the race against the automatic unlock of the encryption session
# (`/search/query` answers 423 content_locked). The Go host waits for the session first, so it is expected to restore where Tauri did not.
if [ "$(xclip -o -selection clipboard 2>/dev/null)" = hello-two ]; then info "login restore baseline: Tauri restored the last history entry"; else info "login restore baseline: Tauri did NOT restore the last history entry" "$(sed 's/\x1b\[[0-9;]*m//g' /tmp/tauri-gui-2.log | grep -m1 'startup restore' | grep -o 'error=.*' | cut -c1-200)"; fi
check "the Tauri build wrote its own autostart entry" test -e /root/.config/autostart/UniClipboard.desktop
cp -a /root/.config/autostart /out/before/autostart; cat /root/.config/autostart/*.desktop > /out/before/autostart.txt
ls /root/.config/autostart | wc -l > /out/before/autostart-count.txt
sleep 3
python3 /w/tauri_continuity_probe.py capture /out/before/capture.json
python3 /w/keyring_inventory.py /out/before/keyring.json /tmp/keyring-salt
python3 /w/localstorage_inventory.py /out/before/localstorage.json
entries=$(python3 -c "import json;d=json.load(open('/out/before/capture.json'));print(len(d['entries']['body']['data']))")
[ "$entries" = 3 ] && ok "the Tauri build captured text and image history" "entries=$entries" || bad "the Tauri build captured text and image history" "entries=$entries"
cp /tmp/tauri-gui-2.log /out/
if [ "$scenario" = stopped ] || [ "$kind" = AppImage ]; then
  stop_all && ok "the Tauri build exits cleanly before the upgrade" || bad "the Tauri build exits cleanly before the upgrade" "processes had to be killed"
  datafiles /out/before/datafiles.json
  # Control: the same Tauri build started once more shows which files change on an ordinary restart, so an upgrade-induced change is separable.
  $OLD_BIN --autostart > /tmp/tauri-gui-3.log 2>&1 &
  check "control: Tauri restarts once more" wait_ready
  sleep 6; stop_all && ok "control: Tauri exits cleanly" || bad "control: Tauri exits cleanly"
  datafiles /out/control/datafiles.json
else
  datafiles /out/before/datafiles.json  # best effort while the daemon runs; the DB bytes are not compared
  ps -eo pid,etime,args | grep -E "uniclip|WebKit" | grep -v grep > /out/before/processes.txt
fi
[ "$kind" = AppImage ] || sha256sum /usr/bin/uniclipboard /usr/bin/uniclipd > /out/before/old-binaries.sha256

# ---- phase 3: the package transaction ----
pm_state > /out/packages-before.txt
pm_install > /out/install.log 2>&1; rc=$?
[ "$rc" = 0 ] && ok "the package manager installs the Go package over the Tauri package ($scenario)" || bad "the package manager installs the Go package over the Tauri package ($scenario)" "rc=$rc $(tail -n 3 /out/install.log | tr '\n' ' ')"
pm_state > /out/packages-after.txt
if [ "$kind" = AppImage ]; then
  if [ "$scenario" = samepath ]; then [ "$(sha256sum $OLD_BIN | cut -d' ' -f1)" != "$old_sha" ] && ok "the Tauri AppImage file has been replaced" || bad "the Tauri AppImage file has been replaced"
  else [ ! -e $OLD_BIN ] && ok "the Tauri AppImage file is gone" || bad "the Tauri AppImage file is gone"; fi
else
  pm_old_present && bad "the Tauri package identity is gone" "$(pm_state | tr '\n' ' ')" || ok "the Tauri package identity is gone"
fi
if [ "$kind" = AppImage ]; then
  (cd /tmp && rm -rf squashfs-root && "$NEW_BIN" --appimage-extract usr/bin/uniclipd >/dev/null 2>&1)
  check "the shipped daemon is the one the build evidence describes" test "$(sha256sum /tmp/squashfs-root/usr/bin/uniclipd | cut -d' ' -f1)" = "$sha"
else
  [ "$(pm_owners /usr/bin/uniclipboard)" = "uniclipboard " ] && ok "exactly one package owns /usr/bin/uniclipboard" || bad "exactly one package owns /usr/bin/uniclipboard" "$(pm_owners /usr/bin/uniclipboard)"
  check "the shipped daemon is the one the build evidence describes" test "$(sha256sum /usr/bin/uniclipd | cut -d' ' -f1)" = "$sha"
fi
if [ "$scenario" = running ]; then
  { pgrep -x uniclipboard >/dev/null && pgrep -x uniclipd >/dev/null; } && ok "the running Tauri processes survived the file replacement" || bad "the running Tauri processes survived the file replacement"
  ps -eo pid,etime,args | grep -E "uniclip|WebKit" | grep -v grep > /out/after/processes-after-install.txt
  stop_all && ok "the old processes stop after the upgrade" || bad "the old processes stop after the upgrade" "processes had to be killed"
fi
info "autostart entries after the package transaction" "$(ls /root/.config/autostart | tr '\n' ' ')"
cp -a /root/.config/autostart /out/after/autostart-after-install; cat /root/.config/autostart/*.desktop > /out/after/autostart-after-install.txt

# ---- phase 4: the Go host starts the way the login manager starts it ----
mapfile -t entry_args < <(python3 - <<'PY'
import shlex
from pathlib import Path
lines = Path("/root/.config/autostart/UniClipboard.desktop").read_text().splitlines()
print("\n".join(shlex.split(next(l[5:] for l in lines if l.startswith("Exec=")))))
PY
)
printf '%s\n' "${entry_args[@]}" > /out/after/autostart-exec.txt
if [ "$kind" = AppImage ] && [ "$scenario" = newpath ]; then
  [ ! -e "${entry_args[0]}" ] && ok "newpath: the Tauri-written entry is stale until the Go AppImage runs (its Exec file is gone)" "${entry_args[0]}" || bad "newpath: the Tauri-written entry is stale until the Go AppImage runs" "${entry_args[0]}"
  entry_args=("$NEW_BIN")  # the user starts the downloaded AppImage by hand; the entry is repaired by that run
fi
export UC_GUI_GO_E2E_PHASE=startup-observe
pkill -x xclip 2>/dev/null; sleep 1; [ -z "$(xclip -o -selection clipboard 2>/dev/null)" ] && ok "the X11 clipboard is empty before the Go login start" || bad "the X11 clipboard is empty before the Go login start"
"${entry_args[@]}" > /tmp/go-gui-1.log 2>&1 &
gui=$!
check "the Go GUI, daemon and WebView start from the Tauri-written entry" wait_ready
sleep 8
{ echo "gui_exe=$(readlink /proc/$gui/exe 2>/dev/null)"; echo "daemon_exe=$(readlink /proc/$(pgrep -x uniclipd | head -1)/exe 2>/dev/null)"; } > /out/after/launch.txt
if [ "$kind" = AppImage ]; then
  info "AppImage processes" "$(cat /out/after/launch.txt | tr '\n' ' ')"
else
  grep -q '^gui_exe=/usr/bin/uniclipboard$' /out/after/launch.txt && ok "GUI runs from the installed path" || bad "GUI runs from the installed path" "$(cat /out/after/launch.txt)"
  grep -q '^daemon_exe=/usr/bin/uniclipd$' /out/after/launch.txt && ok "daemon runs from the installed path" || bad "daemon runs from the installed path"
fi
[ "$(xclip -o -selection clipboard 2>/dev/null)" = hello-two ] && ok "login restore: the Go host puts the last history entry back on the clipboard" || bad "login restore: the Go host puts the last history entry back on the clipboard" "clipboard=$(xclip -o -selection clipboard 2>/dev/null | head -c 40)"
python3 /w/tauri_continuity_probe.py capture /out/after/capture.json
python3 /w/keyring_inventory.py /out/after/keyring.json /tmp/keyring-salt
python3 /w/localstorage_inventory.py /out/after/localstorage.json
cp /tmp/go-gui-1.log /out/
stop_all && ok "the Go host exits cleanly" || bad "the Go host exits cleanly" "processes had to be killed"
datafiles /out/after/datafiles.json
cp -a /root/.config/autostart /out/after/autostart; cat /root/.config/autostart/*.desktop > /out/after/autostart.txt
ls /root/.config/autostart | wc -l > /out/after/autostart-count.txt
[ "$(cat /out/after/autostart-count.txt)" = 1 ] && ok "exactly one autostart entry after the upgrade" || bad "exactly one autostart entry after the upgrade" "$(ls /root/.config/autostart | tr '\n' ' ')"
if [ "$kind" = AppImage ] && [ "$scenario" = newpath ]; then
  grep -qxF "Exec=$NEW_BIN --autostart" /root/.config/autostart/UniClipboard.desktop && ok "newpath: the Go AppImage repaired the entry to its own path" "$(grep '^Exec=' /root/.config/autostart/UniClipboard.desktop)" || bad "newpath: the Go AppImage repaired the entry to its own path" "$(grep '^Exec=' /root/.config/autostart/UniClipboard.desktop)"
  mapfile -t entry_args < <(python3 -c "
import shlex
from pathlib import Path
l = Path('/root/.config/autostart/UniClipboard.desktop').read_text().splitlines()
print('\n'.join(shlex.split(next(x[5:] for x in l if x.startswith('Exec=')))))")
else
  cmp -s /out/before/autostart.txt /out/after/autostart.txt && ok "the autostart entry is byte-identical after the Go host has run" || bad "the autostart entry is byte-identical after the Go host has run"
fi

# second start: the migrated profile must survive a restart of the Go host
"${entry_args[@]}" > /tmp/go-gui-2.log 2>&1 &
check "the Go host restarts on the migrated profile" wait_ready
sleep 5
mkdir -p /out/restart
python3 /w/tauri_continuity_probe.py capture /out/restart/capture.json
python3 /w/keyring_inventory.py /out/restart/keyring.json /tmp/keyring-salt
cp /tmp/go-gui-2.log /out/
stop_all && ok "the Go host exits cleanly after the restart" || bad "the Go host exits cleanly after the restart"
datafiles /out/restart/datafiles.json

# ---- phase 5: comparison ----
python3 /w/tauri_continuity_compare.py /out/before /out/after /out/compare.tsv /out/control /out/restart | sed 's/^/compare: /' >> /out/commands.log
cat /out/compare.tsv >> "$results"
SESSION
rc=$?
fail_count=$(grep -c '^FAIL' "$results" || true)
echo "inner session rc=$rc failures=$fail_count"
exit "$fail_count"
