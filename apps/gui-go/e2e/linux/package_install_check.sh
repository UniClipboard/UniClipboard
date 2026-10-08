#!/usr/bin/env bash
# Real package-manager lifecycle of the deb or rpm in a throwaway container of the distribution under test
# (docs/architecture/gui-go-linux-ci-packaging.md, C11/C14).
#
#   package_install_check.sh <deb|rpm> <image> <fresh|upgrade|reject-newer> <new package> <old package> <expected daemon sha256> <outdir>
#
# fresh:   install the new package -> launch the shipped form -> remove.
# upgrade: install the legacy-name Tauri or Go package -> install the new package over it with the package manager the user
#          would use -> launch -> remove. The old package's files that the new one does not ship must be gone, and exactly one package
#          may own /usr/bin/uniclipboard afterwards.
# reject-newer (rpm): reject a lower-version renamed candidate without changing the installed legacy package.
# Every assertion is a line in <outdir>/results.tsv (PASS|FAIL<TAB>name<TAB>detail); the exit status is the number of failures.
# Container evidence only: headless Xvfb, an unlocked throwaway gnome-keyring as the Secret Service, no desktop environment, no login session.
set -uo pipefail
if [ "${1:-}" != "--inner" ]; then
  kind="${1:?deb|rpm}"; image="${2:?image}"; scenario="${3:?fresh|upgrade}"; new="${4:?new package}"; old="${5:?old package}"; sha="${6:?daemon sha256}"; out="${7:?outdir}"
  [ ! -e "$out" ] || { echo "output already exists: $out" >&2; exit 2; }
  case "$kind:$scenario" in deb:fresh|deb:upgrade|rpm:fresh|rpm:upgrade|rpm:reject-newer) ;; *) exit 2 ;; esac
  mkdir -p "$out"; out="$(cd "$out" && pwd)"
  here="$(cd "$(dirname "$0")" && pwd)"
  # Freeze the harness before running; a shared checkout may change during a long package-manager transaction.
  mkdir "$out/source"
  cp "$here/package_install_check.sh" "$here/package_profile_check.py" "$out/source/"
  shasum -a 256 "$out/source/"* > "$out/harness.sha256"
  platform="${UC_DOCKER_PLATFORM:?UC_DOCKER_PLATFORM}"
  docker run --rm --init --cap-add IPC_LOCK --platform "$platform" -v "$out/source/package_install_check.sh:/check.sh:ro" -v "$out/source/package_profile_check.py:/profile.py:ro" -v "$new:/in/new.$kind:ro" -v "$old:/in/old.$kind:ro" -v "$out:/out" \
    "$image" bash /check.sh --inner "$kind" "$scenario" "$sha" > "$out/run.log" 2>&1
  rc=$?
  tail -n 40 "$out/results.tsv" 2>/dev/null; exit "$rc"
fi

shift; kind="$1"; scenario="$2"; sha="$3"
sha256sum /in/new.* /in/old.* > /out/package-inputs.sha256
results=/out/results.tsv; : > "$results"; failures=0
ok()  { printf 'PASS\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
bad() { printf 'FAIL\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; failures=$((failures + 1)); }
check() { local name="$1"; shift; local rc=0; "$@" >/tmp/check.out 2>&1 || rc=$?; { printf "\n%s: " "$name"; printf "%q " "$@"; printf "\n"; cat /tmp/check.out; } >> /out/commands.log; if [ "$rc" = 0 ]; then ok "$name"; else bad "$name" "$(tr '\n' ' ' < /tmp/check.out | cut -c1-300)"; fi; }
{ echo "image: $(. /etc/os-release && echo "$PRETTY_NAME")"; echo "machine: $(uname -m)"; ldd --version | head -1; } | tee /out/host.txt

if [ "$kind" = deb ]; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq && apt-get install -y -qq --no-install-recommends ca-certificates >/dev/null
  tools=(xvfb xauth dbus dbus-x11 gnome-keyring libsecret-tools procps python3)
  # The distribution mirrors drop requests now and then; a retry of the package manager is not a retry of the assertion.
  install_pkg() { for a in 1 2 3; do apt-get install -y -qq --no-install-recommends -o Acquire::Retries=5 "$1" && return 0; sleep 5; apt-get update -qq; done; return 1; }
  remove_pkg()  { apt-get remove -y -qq uniclipboard; }
  installed()   { dpkg-query -W -f '${Status} ${Version}\n' uniclipboard 2>/dev/null; }
  is_installed() { dpkg-query -W -f '${db:Status-Abbrev}' uniclipboard 2>/dev/null | grep -q '^ii'; }
  list_files()  { dpkg -L uniclipboard; }
  owners()      { dpkg -S "${1:-/usr/bin/uniclipboard}" 2>/dev/null | cut -d: -f1 | sort -u | tr '\n' ' '; }
  verify_files() { dpkg --verify uniclipboard; }
  apt-get install -y -qq --no-install-recommends "${tools[@]}" >/dev/null
else
  dnf -y -q install xorg-x11-server-Xvfb xauth dbus-x11 gnome-keyring libsecret procps-ng python3 cpio diffutils >/dev/null
  install_pkg() { for a in 1 2 3; do dnf -y -q --setopt=retries=10 install "$1" && return 0; sleep 5; done; return 1; }
  remove_pkg()  { dnf -y -q remove uniclipboard; }
  installed()   { rpm -q --qf '%{NAME} %{VERSION}-%{RELEASE}\n' uniclipboard 2>/dev/null; }
  is_installed() { rpm -q uniclipboard >/dev/null 2>&1; }
  list_files()  { rpm -ql uniclipboard; }
  owners()      { rpm -qf --qf '%{NAME}\n' "${1:-/usr/bin/uniclipboard}" 2>/dev/null | sort -u | tr '\n' ' '; }
  verify_files() { rpm -V uniclipboard; }
fi

# Identical payloads can be co-owned by RPM; the version conflict must reject the transaction before that happens.
if [ "$scenario" = reject-newer ]; then
  check "newer legacy package installs" install_pkg /in/old.rpm
  rpm -qa --qf '%{NAME} %{VERSION}-%{RELEASE}\n' | sort > /out/packages-before.txt
  sha256sum /usr/bin/uniclipboard /usr/bin/uniclipd > /out/payload-before.sha256
  if dnf -y -q install /in/new.rpm > /out/rejected-transaction.log 2>&1; then bad "newer legacy prevents coinstallation"; else ok "newer legacy prevents coinstallation"; fi
  check "rejection reports the legacy version conflict" grep -F "conflicts with uni-clipboard >" /out/rejected-transaction.log
  rpm -qa --qf '%{NAME} %{VERSION}-%{RELEASE}\n' | sort > /out/packages-after.txt
  check "rejection leaves package database unchanged" cmp /out/packages-before.txt /out/packages-after.txt
  check "rejection leaves payload unchanged" sha256sum -c /out/payload-before.sha256
  check "legacy remains sole file owner" bash -c '[ "$(rpm -qf --qf "%{NAME}\n" /usr/bin/uniclipboard)" = uni-clipboard ]'
  exit "$failures"
fi

old_desktop=/usr/share/applications/UniClipboard.desktop; new_desktop=/usr/share/applications/uniclipboard.desktop
if [ "$scenario" = upgrade ]; then
  check "old package installs" install_pkg /in/old.$kind
  if [ "$kind" = deb ]; then dpkg-query -W uni-clipboard > /out/old-package.txt; dpkg -L uni-clipboard > /tmp/old-files; else rpm -q uni-clipboard > /out/old-package.txt; rpm -ql uni-clipboard > /tmp/old-files; fi
  ok "old package state" "$(cat /out/old-package.txt)"
  # A released Tauri package has no sibling daemon; Go uses its actual installed daemon.
  if [ -e /usr/bin/uniclipd ] && [ ! -e "$old_desktop" ]; then seed_daemon=/usr/bin/uniclipd; else
    mkdir -p /tmp/seed
    if [ "$kind" = deb ]; then dpkg-deb -x /in/new.deb /tmp/seed; else (cd /tmp/seed && rpm2cpio /in/new.rpm | cpio -id --quiet); fi
    seed_daemon=/tmp/seed/usr/bin/uniclipd
  fi
fi
# Shipped form: no profile, the real data root of this throwaway root user, the Secret Service of an unlocked throwaway keyring.
launch() {
  export UC_PACKAGE_PROFILE_MODE="$1" UC_PACKAGE_SEED_DAEMON="${seed_daemon:-/usr/bin/uniclipd}"
  export HOME=/root; mkdir -p /root/.local/share /root/.config /root/.cache  # gnome-keyring needs ~/.cache for its sockets
  dbus-run-session -- bash -c '
    set -u
    printf "uc-throwaway" | gnome-keyring-daemon --foreground --unlock --components=secrets > /tmp/keyring.env 2> /tmp/keyring.err &
    for i in $(seq 100); do dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true" && break; sleep 0.1; done
    printf "probe" | secret-tool store --label=uc-probe uc probe && [ "$(secret-tool lookup uc probe)" = probe ] && secret-tool clear uc probe && echo keyring-ready
    trap "kill \${gui:-} \${xvfb:-} 2>/dev/null; pkill -x uniclipd 2>/dev/null || true" EXIT
    Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & xvfb=$!
    export DISPLAY=:99; for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
    if [ "$UC_PACKAGE_PROFILE_MODE" = seed ]; then
      "$UC_PACKAGE_SEED_DAEMON" > /tmp/gui.log 2>&1 & gui=$!
    else
      export UC_GUI_GO_E2E_PHASE=startup-observe
      export UC_GUI_GO_EXIT_MODE=full
      mapfile -t entry_args < <(python3 /profile.py exec)
      [ "${#entry_args[@]}" = 2 ] || exit 1
      "${entry_args[@]}" > /tmp/gui.log 2>&1 & gui=$!
    fi
    daemon=""; webkit=""
    for i in $(seq 90); do
      daemon=$(pgrep -x uniclipd | head -1); webkit=$(pgrep -f "[W]ebKitWebProcess" | head -1)
      [ -n "$daemon" ] && { [ "$UC_PACKAGE_PROFILE_MODE" = seed ] || [ -n "$webkit" ]; } && break; sleep 1
    done
    python3 /profile.py "$UC_PACKAGE_PROFILE_MODE" || exit 1
    if [ "$UC_PACKAGE_PROFILE_MODE" = seed ]; then
      printf "package-key-sentinel" | secret-tool store --label=package-fixture fixture package-migration
    else
      [ "$(secret-tool lookup fixture package-migration)" = package-key-sentinel ] || exit 1
    fi
    sleep 10
    { echo "gui_exe=$(readlink /proc/$gui/exe 2>/dev/null)"; echo "daemon_exe=$(readlink /proc/$daemon/exe 2>/dev/null)"; echo "webkit_exe=$(readlink /proc/$webkit/exe 2>/dev/null)"
      echo "gui_alive=$(kill -0 $gui 2>/dev/null && echo yes || echo no)"; echo "daemon_alive=$([ -n "$daemon" ] && kill -0 $daemon 2>/dev/null && echo yes || echo no)"
      echo "daemon_conn=$(ls /root/.local/share/*/daemon.conn /root/.local/share/*/*/daemon.conn 2>/dev/null | head -1)"; } > /tmp/launch.txt
    ps -eo pid,ppid,etime,args | grep -E "uniclip|WebKit" | grep -v grep > /tmp/launch-ps.txt
    kill $gui 2>/dev/null; sleep 3; echo "daemon_after_gui_exit=$(pgrep -x uniclipd >/dev/null && echo running || echo gone)" >> /tmp/launch.txt
    pkill -x uniclipd 2>/dev/null; for i in $(seq 50); do pgrep -x uniclipd >/dev/null || break; sleep .2; done; kill $xvfb 2>/dev/null; true'
  local launch_rc=$?
  cp /tmp/gui.log "/out/$UC_PACKAGE_PROFILE_MODE-gui.log" 2>/dev/null || true
  cp /tmp/launch.txt /tmp/launch-ps.txt /tmp/gui.log /out/ 2>/dev/null
  return "$launch_rc"
}
snapshot() {
  (cd /root && find .local/share/app.uniclipboard.desktop .local/share/keyrings .config/autostart -type f \
    ! -name '*.sqlite-wal' ! -name '*.sqlite-shm' ! -name 'daemon*.conn' ! -name '.daemon-pid' ! -name '.uniclipd.lock' ! -name 'daemon-run.json*' \
    -print0 | sort -z | xargs -0 sha256sum) > "$1"
}
if [ "$scenario" = upgrade ]; then
  check "seed a real encrypted profile and enabled autostart" launch seed
  snapshot /tmp/before.sha256
fi
check "new package installs ($scenario)" install_pkg /in/new.$kind
ok "new package state" "$(installed)"
[ "$(owners)" = "uniclipboard " ] && ok "one owner of /usr/bin/uniclipboard" "$(owners)" || bad "one owner of /usr/bin/uniclipboard" "$(owners)"
check "installed files verify against the package database" verify_files
check "the daemon is the evidence daemon" test "$(sha256sum /usr/bin/uniclipd | cut -d' ' -f1)" = "$sha"
check "no unresolved library in the GUI executable" bash -c '! ldd /usr/bin/uniclipboard | grep "not found"'
check "no unresolved library in the daemon" bash -c '! ldd /usr/bin/uniclipd | grep "not found"'
check "the new desktop entry is installed" test -e "$new_desktop"
check "the retired Tauri desktop entry is absent" test ! -e "$old_desktop"


if [ "$kind" = deb ]; then
  ! dpkg-query -W -f '${db:Status-Abbrev}' uni-clipboard 2>/dev/null | grep -q '^ii' && ok "legacy package identity is absent" || bad "legacy package identity is absent"
  reinstall_pkg() { apt-get install --reinstall -y -qq /in/new.deb; }
else
  ! rpm -qa --qf '%{NAME}\n' | grep -qx uni-clipboard && ok "legacy package identity is absent" || bad "legacy package identity is absent"
  legacy_isa="uni-clipboard$(rpm --eval '%{?_isa}')"
  check "legacy architecture dependency alias resolves to the new identity" bash -c 'test "$(rpm -q --whatprovides "$1" --qf "%{NAME}")" = uniclipboard' _ "$legacy_isa"
  reinstall_pkg() { dnf -y -q reinstall /in/new.rpm; }
fi
if [ "$scenario" = upgrade ]; then
  snapshot /tmp/after.sha256
  check "package transaction preserves encrypted profile and enabled entry bytes" cmp /tmp/before.sha256 /tmp/after.sha256
  # Every retired regular file must be removed, not merely dropped from the package database.
  while IFS= read -r path; do
    [ -f "$path" ] || continue
    if [ "$kind" = deb ]; then dpkg -S "$path" >/dev/null 2>&1; else rpm -qf "$path" >/dev/null 2>&1; fi
    [ "$?" = 0 ] || bad "retired package file remains" "$path"
  done < /tmp/old-files
else
  seed_daemon=/usr/bin/uniclipd
  check "seed a real encrypted profile and enabled autostart" launch seed
  snapshot /tmp/before.sha256
fi
check "repeated install is idempotent" install_pkg /in/new.$kind
check "explicit reinstall succeeds" reinstall_pkg
snapshot /tmp/reinstalled.sha256
check "reinstall preserves encrypted profile and enabled entry bytes" cmp /tmp/before.sha256 /tmp/reinstalled.sha256
check "reinstalled files verify" verify_files
list_files > /tmp/new-files
owner_failures=$failures
while IFS= read -r path; do
  [ -f "$path" ] || continue
  [ "$(owners "$path")" = "uniclipboard " ] || bad "new package file has unexpected owners" "$path: $(owners "$path")"
done < /tmp/new-files
[ "$failures" != "$owner_failures" ] || ok "all new regular files have one new package owner"
launch verify > /out/launch-stdout.txt 2>&1
launch_rc=$?
[ "$launch_rc" = 0 ] && ok "initialized profile and keyring survive restart" || bad "initialized profile and keyring survive restart" "exit=$launch_rc"

. /tmp/launch.txt 2>/dev/null
grep -q '^keyring-ready$' /out/launch-stdout.txt && ok "the Secret Service of the harness is ready" || bad "the Secret Service of the harness is ready" "$(grep -i -m1 'keyring\|secrets' /out/launch-stdout.txt)"
[ "${gui_exe:-}" = /usr/bin/uniclipboard ] && ok "GUI runs from the installed path" || bad "GUI runs from the installed path" "exe=${gui_exe:-none}"
[ "${daemon_exe:-}" = /usr/bin/uniclipd ] && ok "daemon runs from the installed path" || bad "daemon runs from the installed path" "exe=${daemon_exe:-none}"
case "${webkit_exe:-}" in /usr/lib/*/webkit2gtk-4.1/WebKitWebProcess|/usr/lib64/webkit2gtk-4.1/WebKitWebProcess|/usr/libexec/webkit2gtk-4.1/WebKitWebProcess) ok "WebView process runs from the installed WebKitGTK" "$webkit_exe" ;; *) bad "WebView process runs from the installed WebKitGTK" "exe=${webkit_exe:-none}" ;; esac
[ "${gui_alive:-}" = yes ] && [ "${daemon_alive:-}" = yes ] && ok "GUI and daemon are still alive 10 s after the WebView appeared" || bad "GUI and daemon are still alive 10 s after the WebView appeared" "gui=${gui_alive:-?} daemon=${daemon_alive:-?}"
[ -n "${daemon_conn:-}" ] && ok "daemon published its connection file" "$daemon_conn" || bad "daemon published its connection file" "none under /root/.local/share"

snapshot /tmp/pre-remove.sha256
check "the package removes" remove_pkg
snapshot /tmp/post-remove.sha256
check "removal preserves encrypted profile and enabled entry bytes" cmp /tmp/pre-remove.sha256 /tmp/post-remove.sha256
while IFS= read -r path; do
  [ ! -f "$path" ] || bad "owned file survives removal" "$path"
done < /tmp/new-files
check "the executables are gone after removal" bash -c '! test -e /usr/bin/uniclipboard && ! test -e /usr/bin/uniclipd'
check "the desktop entry is gone after removal" bash -c "! test -e $new_desktop"
! is_installed && ok "the package manager reports it as not installed" || bad "the package manager reports it as not installed" "$(installed)"
cp /tmp/*sha256 /out/
check "user data survives a plain removal" test -d /root/.local/share/app.uniclipboard.desktop
echo "failures=$failures" | tee -a "$results"
exit "$failures"
