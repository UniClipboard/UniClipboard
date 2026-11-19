#!/bin/sh
set -eu
cd "$(git rev-parse --show-toplevel)"
export TMPDIR=/private/tmp
dest=${1:-.cache/observability-sync-followup/replay}
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
bin=$(mktemp -d /private/tmp/unic-sync-followup.XXXXXX)
trap 'rm -rf "$bin"' EXIT
go build -race -tags unic_e2e -o "$bin/unicd" ./app/service/cmd/unicd
go build -race -o "$bin/unic" ./app/service/cmd/unic
UNIC_E2E=1 UNIC_E2E_BIN="$bin" UNIC_E2E_ARTIFACTS="$dest/race" \
 go test -race ./e2e/cli \
 -run '^(TestObservability(|Pipeline|Faults)|TestPostHog(|Transport|EnableFromDisabled)|TestSyncObservability(|Faults|RecoveryFollowup|DailyFollowup|HandshakeFollowup|CrashFollowup|SourceFollowup|ExternalFollowup))$' \
 -count=1 -v
go build -tags unic_e2e -o "$bin/unicd" ./app/service/cmd/unicd
go build -o "$bin/unic" ./app/service/cmd/unic
UNIC_E2E=1 UNIC_E2E_BIN="$bin" UNIC_E2E_ARTIFACTS="$dest/cost" \
 go test -race ./e2e/cli -run '^TestSyncObservabilityCostFollowup$' -count=1 -v
