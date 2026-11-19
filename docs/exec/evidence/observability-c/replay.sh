#!/bin/sh
set -eu

cd "$(git rev-parse --show-toplevel)"
export TMPDIR=/private/tmp
dest=${1:-.cache/obs-c/replay}
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
bin=$(mktemp -d /private/tmp/unic-c-replay.XXXXXX)
trap 'rm -rf "$bin"' EXIT

go build -race -tags unic_e2e -o "$bin/unicd" ./app/service/cmd/unicd
go build -race -o "$bin/unic" ./app/service/cmd/unic
UNIC_E2E=1 UNIC_E2E_BIN="$bin" UNIC_E2E_ARTIFACTS="$dest/local" \
  go test -race ./e2e/cli -run '^(TestObservability.*|TestSyncObservability(|Faults|Payloads))$' -count=1 -v

go build -tags unic_e2e -o "$bin/unicd" ./app/service/cmd/unicd
go build -o "$bin/unic" ./app/service/cmd/unic
UNIC_E2E=1 UNIC_E2E_BIN="$bin" UNIC_E2E_ARTIFACTS="$dest/sampling" \
  go test -race ./e2e/cli -run '^TestSyncObservabilitySampling$' -count=1 -v

go build -race -tags ts_omit_captiveportal,ts_omit_portmapper -o "$bin/unicd" ./app/service/cmd/unicd
UNIC_SYNC_PATH_E2E=1 UNIC_SYNC_PATH_BIN="$bin" UNIC_SYNC_PATH_ARTIFACTS="$dest/real-path" \
  go test -race ./e2e/syncpath -run '^TestPassivePaths$' -count=1 -v
