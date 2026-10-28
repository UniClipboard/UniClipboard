#!/usr/bin/env bash
# Run a command with a GPG home in non-swappable RAM, isolated from host mounts.
set -euo pipefail
exec unshare --user --map-root-user --mount --propagation private bash -c '
  set -euo pipefail
  memory=$(mktemp -d /tmp/uc-gpg-XXXXXXXX)
  cleanup() {
    if mountpoint -q "$memory"; then
      GNUPGHOME="$memory" gpgconf --kill all >/dev/null 2>&1 || true
      # GPG agent shutdown is asynchronous; detach RAM while it finishes exiting.
      umount --lazy "$memory"
    fi
    rmdir "$memory"
  }
  trap cleanup EXIT
  mount -t ramfs -o mode=0700 ramfs "$memory"
  export GNUPGHOME="$memory"
  "$@"
' _ "$@"
