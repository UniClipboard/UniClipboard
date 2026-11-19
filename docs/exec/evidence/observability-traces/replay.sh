#!/bin/sh
set -eu
cd "$(git rev-parse --show-toplevel)"
export TMPDIR=/private/tmp
dest=${1:-.cache/observability-traces/replay}
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
bin=$(mktemp -d /private/tmp/unic-traces-replay.XXXXXX)
trap 'rm -rf "$bin"' EXIT
go build -o "$bin/unic" ./app/service/cmd/unic
go build -o "$bin/unicd" ./app/service/cmd/unicd
UNIC_E2E=1 UNIC_E2E_BIN="$bin" UNIC_TRACE_RACE=1 \
 UNIC_E2E_ARTIFACTS="$dest/race" \
 go test -race ./e2e/cli -run '^TestTraces$' -count=1 -v
UNIC_E2E=1 UNIC_E2E_BIN="$bin" UNIC_TRACE_RACE=0 \
 UNIC_E2E_ARTIFACTS="$dest/cost" \
 go test -race ./e2e/cli -run '^TestTracesCostAndSampling$' -count=1 -v
go run ./e2e/tracecloud --local --artifact "$dest/local-driver"
