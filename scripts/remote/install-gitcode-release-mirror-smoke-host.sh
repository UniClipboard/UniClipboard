#!/bin/sh
# Installs the SMOKE-ONLY host side of mkdir700/gitcode-release-mirror on the
# Shanghai mirror host. Review before running; it is meant to be run as root
# by a maintainer. It never touches the production accounts or files:
#   - not /opt/uniclip-mirror/*, not /home/uniclip-mirror/**
#   - not sshd_config, and it does not restart or reload sshd
# It creates its own account, version directory, config directory and Node copy,
# so removing them (see the end of this file) fully undoes it.
#
# Usage:
#   install-gitcode-release-mirror-smoke-host.sh <action-checkout> <ci-public-key-file> <source-commit-sha>...
#
#   <action-checkout>      a checkout of mkdir700/gitcode-release-mirror at the pinned commit
#   <ci-public-key-file>   the PUBLIC half of the key dedicated to the smoke test
#   <source-commit-sha>    full SHA(s) of the UniClipboard commit(s) the smoke workflow will run
#                          from; only raw.githubusercontent.com paths under that SHA's
#                          .github/gitcode-action-smoke/ are fetchable
set -eu

PINNED_ACTION_COMMIT=5055ef17b7302e27ca53cbe5f6e44722f7e55f6b
PINNED_CORE_SHA256=b0fd51012e0f0330c72ef45d25a60a3ac91b19bb6d64d6ffaeca8181d22ee8d8
ACCOUNT=gitcode-smoke
PREFIX=/opt/gitcode-release-mirror-smoke-5055ef1
CONFIG_DIR=/etc/gitcode-release-mirror-smoke
GITCODE_REPOSITORY=UniClipboard/UniClipboard
NODE_SOURCE=${NODE_SOURCE:-/home/uniclip-mirror/node/bin/node}   # read-only use, copied

[ "$(id -u)" = 0 ] || { echo "run as root" >&2; exit 1; }
[ $# -ge 3 ] || { echo "usage: $0 <action-checkout> <ci-public-key-file> <source-commit-sha>..." >&2; exit 2; }
checkout=$1; pubkey=$2; shift 2

head=$(cd "$checkout" && git rev-parse HEAD)
[ "$head" = "$PINNED_ACTION_COMMIT" ] || { echo "checkout is $head, expected $PINNED_ACTION_COMMIT" >&2; exit 1; }
core=$(sha256sum "$checkout/lib/mirror-core.mjs" | cut -d' ' -f1)
[ "$core" = "$PINNED_CORE_SHA256" ] || { echo "mirror-core.mjs is $core, expected $PINNED_CORE_SHA256" >&2; exit 1; }
[ "$(wc -l < "$pubkey")" -le 1 ] && grep -q '^ssh-ed25519 ' "$pubkey" || { echo "key file must hold one ssh-ed25519 public key" >&2; exit 1; }
for sha in "$@"; do
  printf '%s' "$sha" | grep -Eq '^[0-9a-f]{40}$' || { echo "not a full commit SHA: $sha" >&2; exit 1; }
done

if ! id "$ACCOUNT" >/dev/null 2>&1; then
  useradd --system --create-home --shell /bin/sh "$ACCOUNT"
  passwd -l "$ACCOUNT" >/dev/null
fi

# Config first, so host/install.sh keeps it instead of writing the example.
install -d -m 755 "$CONFIG_DIR"
prefixes=""
for sha in "$@"; do
  prefixes="$prefixes${prefixes:+, }\"https://raw.githubusercontent.com/UniClipboard/UniClipboard/$sha/.github/gitcode-action-smoke/\""
done
cat > "$CONFIG_DIR/config.json.new" <<JSON
{
  "allowedRepositories": ["$GITCODE_REPOSITORY"],
  "allowedSourcePrefixes": [$prefixes],
  "fileNamePattern": "^gitcode-action-smoke[A-Za-z0-9._-]{0,64}\\\\.txt\$",
  "maxFileBytes": 1048576,
  "maxDeadlineSeconds": 600
}
JSON
install -m 444 -o root "$CONFIG_DIR/config.json.new" "$CONFIG_DIR/config.json"
rm -f "$CONFIG_DIR/config.json.new"

sh "$checkout/host/install.sh" --prefix "$PREFIX" --config-dir "$CONFIG_DIR"
install -m 755 -o root "$NODE_SOURCE" "$PREFIX/node"
"$PREFIX/node" --version

# Dedicated account, dedicated key, forced command pinned to this version and config.
install -d -m 700 -o "$ACCOUNT" -g "$ACCOUNT" "/home/$ACCOUNT/.ssh"
printf 'restrict,command="%s/node %s/host.mjs %s/config.json" %s\n' "$PREFIX" "$PREFIX" "$CONFIG_DIR" "$(cat "$pubkey")" \
  > "/home/$ACCOUNT/.ssh/authorized_keys"
chown "$ACCOUNT:$ACCOUNT" "/home/$ACCOUNT/.ssh/authorized_keys"
chmod 600 "/home/$ACCOUNT/.ssh/authorized_keys"

echo "installed. core sha256: $core"
echo "smoke account: $ACCOUNT; prefix: $PREFIX; config: $CONFIG_DIR/config.json"

# Removal (does not touch anything else):
#   userdel -r gitcode-smoke; rm -rf /opt/gitcode-release-mirror-smoke-5055ef1 /etc/gitcode-release-mirror-smoke
