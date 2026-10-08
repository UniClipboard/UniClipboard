#!/usr/bin/env bash
# Real package-manager lifecycle of the deb or rpm in a throwaway container of the distribution under test
# (docs/architecture/gui-go-linux-ci-packaging.md, C11/C14).
#
#   package_install_check.sh <deb|rpm> <image> <fresh|upgrade> <new package> <old package> <expected daemon sha256> <outdir>
#
# fresh:   install the new package -> launch the shipped form -> remove.
# upgrade: install the previously released package (the Tauri one) -> install the new package over it with the package manager the user
#          would use -> launch -> remove. The old package's files that the new one does not ship must be gone, and exactly one package
#          may own /usr/bin/uniclipboard afterwards.
# Every assertion is a line in <outdir>/results.tsv (PASS|FAIL<TAB>name<TAB>detail); the exit status is the number of failures.
# Container evidence only: headless Xvfb, an unlocked throwaway gnome-keyring as the Secret Service, no desktop environment, no login session.
set -uo pipefail
if [ "${1:-}" != "--inner" ]; then
  kind="${1:?deb|rpm}"; image="${2:?image}"; scenario="${3:?fresh|upgrade}"; new="${4:?new package}"; old="${5:?old package}"; sha="${6:?daemon sha256}"; out="${7:?outdir}"
  mkdir -p "$out"; out="$(cd "$out" && pwd)"
  here="$(cd "$(dirname "$0")" && pwd)"
  platform="${UC_DOCKER_PLATFORM:?UC_DOCKER_PLATFORM}"
  docker run --rm --init --cap-add IPC_LOCK --platform "$platform" -v "$here/package_install_check.sh:/check.sh:ro" -v "$new:/in/new.$kind:ro" -v "$old:/in/old.$kind:ro" -v "$out:/out" \
    "$image" bash /check.sh --inner "$kind" "$scenario" "$sha" > "$out/run.log" 2>&1
  rc=$?
  tail -n 40 "$out/results.tsv" 2>/dev/null; exit "$rc"
fi

shift; kind="$1"; scenario="$2"; sha="$3"
results=/out/results.tsv; : > "$results"; failures=0
ok()  { printf 'PASS\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
bad() { printf 'FAIL\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; failures=$((failures + 1)); }
check() { local name="$1"; shift; if "$@" >/tmp/check.out 2>&1; then ok "$name"; else bad "$name" "$(tr '\n' ' ' < /tmp/check.out | cut -c1-300)"; fi; }
{ echo "image: $(. /etc/os-release && echo "$PRETTY_NAME")"; echo "machine: $(uname -m)"; ldd --version | head -1; } | tee /out/host.txt

if [ "$kind" = deb ]; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq && apt-get install -y -qq --no-install-recommends ca-certificates >/dev/null
  tools=(xvfb xauth dbus dbus-x11 gnome-keyring libsecret-tools procps)
  # The distribution mirrors drop requests now and then; a retry of the package manager is not a retry of the assertion.
  install_pkg() { for a in 1 2 3; do apt-get install -y -qq --no-install-recommends -o Acquire::Retries=5 "$1" && return 0; sleep 5; apt-get update -qq; done; return 1; }
  remove_pkg()  { apt-get remove -y -qq uni-clipboard; }
  installed()   { dpkg-query -W -f '${Status} ${Version}\n' uni-clipboard 2>/dev/null; }
  is_installed() { dpkg-query -W -f '${db:Status-Abbrev}' uni-clipboard 2>/dev/null | grep -q '^ii'; }
  owners()      { dpkg -S /usr/bin/uniclipboard 2>/dev/null | cut -d: -f1 | sort -u | tr '\n' ' '; }
  verify_files() { dpkg --verify uni-clipboard; }
  apt-get install -y -qq --no-install-recommends "${tools[@]}" >/dev/null
else
  dnf -y -q install xorg-x11-server-Xvfb xauth dbus-x11 gnome-keyring libsecret procps-ng >/dev/null
  install_pkg() { for a in 1 2 3; do dnf -y -q --setopt=retries=10 install "$1" && return 0; sleep 5; done; return 1; }
  remove_pkg()  { dnf -y -q remove uni-clipboard; }
  installed()   { rpm -q --qf '%{NAME} %{VERSION}-%{RELEASE}\n' uni-clipboard 2>/dev/null; }
  is_installed() { rpm -q uni-clipboard >/dev/null 2>&1; }
  owners()      { rpm -qf --qf '%{NAME}\n' /usr/bin/uniclipboard 2>/dev/null | sort -u | tr '\n' ' '; }
  verify_files() { rpm -V uni-clipboard; }
fi

old_desktop=/usr/share/applications/UniClipboard.desktop; new_desktop=/usr/share/applications/uniclipboard.desktop
if [ "$scenario" = upgrade ]; then
  check "old package installs" install_pkg /in/old.$kind
  ok "old package state" "$(installed)"
  check "old package ships the old desktop entry" test -e "$old_desktop"
fi
check "new package installs ($scenario)" install_pkg /in/new.$kind
ok "new package state" "$(installed)"
[ "$(owners)" = "uni-clipboard " ] && ok "one owner of /usr/bin/uniclipboard" "$(owners)" || bad "one owner of /usr/bin/uniclipboard" "$(owners)"
check "installed files verify against the package database" verify_files
check "the daemon is the evidence daemon" test "$(sha256sum /usr/bin/uniclipd | cut -d' ' -f1)" = "$sha"
check "no unresolved library in the GUI executable" bash -c '! ldd /usr/bin/uniclipboard | grep "not found"'
check "no unresolved library in the daemon" bash -c '! ldd /usr/bin/uniclipd | grep "not found"'
check "the new desktop entry is installed" test -e "$new_desktop"
[ "$scenario" = upgrade ] && check "the old desktop entry is gone after the upgrade" test ! -e "$old_desktop"

# Shipped form: no profile, the real data root of this throwaway root user, the Secret Service of an unlocked throwaway keyring.
launch() {
  export HOME=/root; rm -rf /root/.local/share /root/.config /root/.cache; mkdir -p /root/.local/share /root/.config /root/.cache  # gnome-keyring needs ~/.cache for its sockets
  dbus-run-session -- bash -c '
    set -u
    printf "uc-throwaway" | gnome-keyring-daemon --foreground --unlock --components=secrets > /tmp/keyring.env 2> /tmp/keyring.err &
    for i in $(seq 100); do dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true" && break; sleep 0.1; done
    printf "probe" | secret-tool store --label=uc-probe uc probe && [ "$(secret-tool lookup uc probe)" = probe ] && secret-tool clear uc probe && echo keyring-ready
    Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & xvfb=$!
    export DISPLAY=:99; for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
    /usr/bin/uniclipboard > /tmp/gui.log 2>&1 & gui=$!
    daemon=""; webkit=""
    for i in $(seq 90); do
      daemon=$(pgrep -x uniclipd | head -1); webkit=$(pgrep -f "[W]ebKitWebProcess" | head -1)
      [ -n "$daemon" ] && [ -n "$webkit" ] && break; sleep 1
    done
    sleep 10
    { echo "gui_exe=$(readlink /proc/$gui/exe 2>/dev/null)"; echo "daemon_exe=$(readlink /proc/$daemon/exe 2>/dev/null)"; echo "webkit_exe=$(readlink /proc/$webkit/exe 2>/dev/null)"
      echo "gui_alive=$(kill -0 $gui 2>/dev/null && echo yes || echo no)"; echo "daemon_alive=$([ -n "$daemon" ] && kill -0 $daemon 2>/dev/null && echo yes || echo no)"
      echo "daemon_conn=$(ls /root/.local/share/*/daemon.conn /root/.local/share/*/*/daemon.conn 2>/dev/null | head -1)"; } > /tmp/launch.txt
    ps -eo pid,ppid,etime,args | grep -E "uniclip|WebKit" | grep -v grep > /tmp/launch-ps.txt
    kill $gui 2>/dev/null; sleep 3; echo "daemon_after_gui_exit=$(pgrep -x uniclipd >/dev/null && echo running || echo gone)" >> /tmp/launch.txt
    pkill -x uniclipd 2>/dev/null; kill $xvfb 2>/dev/null; true'
  cp /tmp/launch.txt /tmp/launch-ps.txt /tmp/gui.log /out/ 2>/dev/null
}
launch > /out/launch-stdout.txt 2>&1
. /tmp/launch.txt 2>/dev/null
grep -q '^keyring-ready$' /out/launch-stdout.txt && ok "the Secret Service of the harness is ready" || bad "the Secret Service of the harness is ready" "$(grep -i -m1 'keyring\|secrets' /out/launch-stdout.txt)"
[ "${gui_exe:-}" = /usr/bin/uniclipboard ] && ok "GUI runs from the installed path" || bad "GUI runs from the installed path" "exe=${gui_exe:-none}"
[ "${daemon_exe:-}" = /usr/bin/uniclipd ] && ok "daemon runs from the installed path" || bad "daemon runs from the installed path" "exe=${daemon_exe:-none}"
case "${webkit_exe:-}" in /usr/lib/*/webkit2gtk-4.1/WebKitWebProcess|/usr/lib64/webkit2gtk-4.1/WebKitWebProcess|/usr/libexec/webkit2gtk-4.1/WebKitWebProcess) ok "WebView process runs from the installed WebKitGTK" "$webkit_exe" ;; *) bad "WebView process runs from the installed WebKitGTK" "exe=${webkit_exe:-none}" ;; esac
[ "${gui_alive:-}" = yes ] && [ "${daemon_alive:-}" = yes ] && ok "GUI and daemon are still alive 10 s after the WebView appeared" || bad "GUI and daemon are still alive 10 s after the WebView appeared" "gui=${gui_alive:-?} daemon=${daemon_alive:-?}"
[ -n "${daemon_conn:-}" ] && ok "daemon published its connection file" "$daemon_conn" || bad "daemon published its connection file" "none under /root/.local/share"

check "the package removes" remove_pkg
check "the executables are gone after removal" bash -c '! test -e /usr/bin/uniclipboard && ! test -e /usr/bin/uniclipd'
check "the desktop entry is gone after removal" bash -c "! test -e $new_desktop"
! is_installed && ok "the package manager reports it as not installed" || bad "the package manager reports it as not installed" "$(installed)"
check "user data survives a plain removal" test -d /root/.local/share/app.uniclipboard.desktop
echo "failures=$failures" | tee -a "$results"
exit "$failures"
