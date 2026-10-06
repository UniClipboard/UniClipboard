#!/usr/bin/env bash
# Runs INSIDE uc-gui-go-linux-runtime:17c4 (no /usr/share/mime on purpose). Usage: probe_pixbuf_mime.sh <AppImage>
# Unpacks the AppImage and loads a PNG through ITS gdk-pixbuf with AppRun's environment:
#   with-bundled-mime   as packaged (usr/share/mime/mime.cache present): must load
#   without-mime        the bundled cache removed: must FAIL ("Couldn't recognize the image file format"), proving the probe tells them apart
set -u
APPIMAGE="${1:?AppImage}"; T="$(mktemp -d)"; cd "$T"
"$APPIMAGE" --appimage-extract >/dev/null 2>&1; R="$T/squashfs-root"
export LD_LIBRARY_PATH="$R/usr/lib" GDK_PIXBUF_MODULE_FILE="$R/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" XDG_DATA_DIRS="$R/usr/share:/usr/share"
echo "host /usr/share/mime: $(ls /usr/share/mime 2>&1 | head -1)"
echo "bundled: $(ls -l "$R/usr/share/mime/mime.cache" 2>&1)"
echo "== with-bundled-mime"; python3 -I "$(dirname "$0")/tools/pixbuf_probe.py" "$R"; a=$?
rm -rf "$R/usr/share/mime"
echo "== without-mime"; python3 -I "$(dirname "$0")/tools/pixbuf_probe.py" "$R"; b=$?
echo "rc with=$a without=$b"
