#!/bin/sh
set -eu

: "${UNIC_POSTHOG_PROJECT_TOKEN:?set the selected project ingestion token}"
: "${UNIC_POSTHOG_PROJECT_ID:?set the selected project ID}"
case "$UNIC_POSTHOG_PROJECT_ID" in *[!0-9]*) exit 1 ;; esac
case "${UNIC_POSTHOG_HOST:-}" in
  https://us.i.posthog.com|https://eu.i.posthog.com) ;;
  *) printf '%s\n' 'Select a PostHog Cloud ingestion host.' >&2; exit 1 ;;
esac
case "$UNIC_POSTHOG_PROJECT_TOKEN" in
  *[!A-Za-z0-9_-]*|phc_) exit 1 ;;
  phc_*) ;;
  *) exit 1 ;;
esac

cd "$(git rev-parse --show-toplevel)"
dest=${1:-.cache/obs-c/cloud-sync}
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
bin=$(mktemp -d)
trap 'rm -rf "$bin"' EXIT
version="acceptance.sync.$(date +%s).$$"
package=github.com/uniclipboard/unic/internal/observability
go build -race -trimpath -tags ts_omit_captiveportal,ts_omit_portmapper \
  -ldflags "-X $package.PostHogHost=$UNIC_POSTHOG_HOST -X $package.PostHogToken=$UNIC_POSTHOG_PROJECT_TOKEN -X $package.ProductVersion=$version" \
  -o "$bin/unicd" ./app/service/cmd/unicd
unset UNIC_POSTHOG_PROJECT_TOKEN
UNIC_SYNC_PATH_CLOUD=1 UNIC_SYNC_PATH_VERSION="$version" UNIC_SYNC_PATH_BIN="$bin" \
  UNIC_SYNC_PATH_ARTIFACTS="$dest" go test -race ./e2e/syncpath -run '^TestCloudSyncPath$' -count=1 -v
