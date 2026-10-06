#!/usr/bin/env bash
# Probe of the AppImage runtime's `.home` portable-home mechanism (slice 17c5, docs/architecture/gui-go-linux-appimage-portable.md "Wails 与成熟机制审计").
# Builds a minimal AppImage with the pinned appimagetool whose AppRun only prints what the runtime exported, then observes: $HOME with and without
# `<AppImage>.home`, $APPIMAGE for a path with spaces/non-ASCII and for a symlink launch, `--appimage-portable-home`, a read-only `.home`,
# and `--appimage-extract-and-run`. Runs INSIDE uc-gui-go-linux-runtime:17c4 (FUSE device, SYS_ADMIN; /cache is the cache volume with the pinned tools).
# Everything printed is the evidence; nothing outside /t is touched.
set -u
mkdir -p /stub; printf '#!/bin/sh\nexit 0\n' > /stub/desktop-file-validate; chmod +x /stub/desktop-file-validate; export PATH=/stub:$PATH
mkdir -p /t/AD/usr/bin && cd /t
cat > AD/AppRun <<'X'
#!/bin/sh
echo "ARGS=$* HOME=$HOME APPIMAGE=$APPIMAGE APPDIR=$APPDIR XDG_CONFIG_HOME=${XDG_CONFIG_HOME-unset} OWD=${OWD-unset} ARGV0=${ARGV0-unset}"
touch "$APPDIR/x" 2>&1 | head -1
X
chmod +x AD/AppRun
printf '[Desktop Entry]\nType=Application\nName=p\nExec=p\nIcon=p\nCategories=Utility;\n' > AD/p.desktop
printf 'x' > AD/p.png
ARCH=aarch64 /cache/tools/appimagetool --appimage-extract-and-run --no-appstream AD /t/probe.AppImage 2>&1 | tail -5
D="/t/dir with space é"; mkdir -p "$D"; cp probe.AppImage "$D/My App.AppImage"; A="$D/My App.AppImage"
export HOME=/t/realhome; mkdir -p $HOME
echo "== plain";   "$A"
mkdir "$A.home"; echo "== .home exists"; "$A"; ls -A "$A.home"
echo "== UC-like portable-home flag"; "$A" --appimage-portable-home; ls -d "$D"/*
ln -s "$A" /t/link; echo "== symlink"; /t/link
echo "== chmod 555 .home"; chmod 555 "$A.home"; "$A"
echo "== extract-and-run"; "$A" --appimage-extract-and-run
