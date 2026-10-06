#!/bin/sh
# Strict apt install for the image build: every failure is propagated, a half-configured package set is never
# reported as success. A plain `apt-get install ... && break` loop followed by `command -v <tool>` is NOT enough:
# when every attempt fails, an earlier unpack can still leave the tool present and the layer is cached as DONE
# (that is how the third build produced a layer with unconfigured libwebkit2gtk-4.1-0).
# usage: apt_install.sh <package>...        (APT_OPTS may add transport options; see the Dockerfile)
set -eu
# Transport: the official Ubuntu sources over HTTPS. Plain HTTP through this host's network path returned 502 and
# connection failures under load (build logs docker-build2/3; isolated measurement: HTTP default rc=100 in 406 s with
# 4 errors, HTTPS default rc=0 in 21 s with 0 errors, one run each). That is a measurement of one path at one time, not
# a proven root cause or a promise of stability, which is why the failure checks below stay in place. Needs the CA
# bundle, installed by the first image layer; APT_HTTPS=0 keeps the original scheme.
if [ "${APT_HTTPS:-1}" = 1 ] && [ -e /etc/ssl/certs/ca-certificates.crt ] && [ -e /etc/apt/sources.list.d/ubuntu.sources ]; then
  sed -i 's#http://\(ports\|archive\|security\)\.ubuntu\.com#https://\1.ubuntu.com#g' /etc/apt/sources.list.d/ubuntu.sources
fi
ok=0
for attempt in 1 2 3 4 5 6 7 8; do
  apt-get update
  # shellcheck disable=SC2086
  if apt-get install -y --no-install-recommends -o Acquire::Retries=5 ${APT_OPTS:-} "$@" \
     && dpkg --configure -a && [ -z "$(dpkg --audit)" ]; then
    ok=1
    break
  fi
  echo "apt attempt $attempt failed; repairing and retrying" >&2
  dpkg --configure -a || true
  apt-get install -y -f -o Acquire::Retries=5 ${APT_OPTS:-} || true
  sleep 5
done
[ "$ok" = 1 ] || { echo "apt install failed after all attempts: $*" >&2; exit 1; }
apt-get check
rm -rf /var/lib/apt/lists/*
