#!/bin/sh
# Strict acceptance of the 17c12 image: real proxy, control client, proxy-config stack present; the toolkit libraries are still absent from the host.
set -eu
for tool in tinyproxy curl ip gsettings dconf proxy python3 openssl; do command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }; done
for lib in libwebkit2gtk libgtk-3 libsoup; do
  if ldconfig -p | grep -q "$lib"; then echo "host unexpectedly provides $lib" >&2; exit 1; fi
done
mods=$(ls /usr/lib/*/gio/modules/ | tr '\n' ' ')
for m in libgiolibproxy.so libgiognomeproxy.so libgiognutls.so libdconfsettings.so; do
  case " $mods " in *" $m "*) ;; *) echo "host GIO module missing: $m (have: $mods)" >&2; exit 1;; esac
done
ls /usr/share/glib-2.0/schemas/org.gnome.system.proxy.gschema.xml >/dev/null
echo "proxy image ok: $(. /etc/os-release; echo "$PRETTY_NAME"); $(tinyproxy -v 2>&1 | head -1); $(curl --version | head -1); gio modules: $mods"
