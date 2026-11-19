# Command ledger

Run from the repository root. Each receipt binds its frozen input inventory, toolchain, program SHA-256, suite parameters, request keys, assertions and cleanup. Host credentials stay in OpenSSH configuration. Public paths below replace only the random task-owned temporary prefix. No command installs a service or uses the system clipboard.

## Controller commands

```sh
git rev-parse --show-toplevel
git remote -v
git branch --show-current
git rev-parse HEAD
ssh -o BatchMode=yes -o ConnectTimeout=12 primaryserver \
  'uname -srm; getconf _NPROCESSORS_ONLN; df -Pk /tmp; command -v go; command -v python3; ps -eo comm | sort | uniq -c | sort -nr | head -12'
python3 tools/directorye2e/run.py --output .cache/f-directory/process-final
python3 tools/directorye2e/run.py \
  --run '^TestCLI/(directory-receive-race|directory-mixed-auto|directory-cli-export)$' \
  --output .cache/f-directory/clipboard-race-2
go build -o .cache/f-directory/unic-e2e-final ./e2e/real
.cache/f-directory/unic-e2e-final run \
  --lab e2e/real/directory.local.toml --out .cache/f-directory/full-2
make headless-e2e
TMPDIR=/private/tmp go test ./e2e/cli -run '^TestCatalog$' -update
go vet ./e2e/real ./internal/localapi
GOTOOLCHAIN=go1.27.3 go run github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0 \
  run ./e2e/real/... ./internal/localapi/...
GOTOOLCHAIN=go1.27.3 go run github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0 \
  fmt --diff --diff-colored=false
go run ./tools/docscheck
```

The initial minimal inventory uses `scenarios = ["pairwise"]` and `directory_bytes = [0]`. Full inventories use `[0, 1024, 16777216]`, one repetition, 180 s receives and `["pairwise", "directory"]`. All other payload arrays are empty. `mac` is local; `linux` uses SSH alias `primaryserver`. Local fault controls use two local nodes and explicit `--allow-shared-hosts`. They never count as two-machine evidence. The replay script accepts another authorized alias and creates a fresh inventory.

## Production commands inside owned workers

Workers start `UNIC_HOME=<owned>/state UNIC_SOCK=<owned>/s <owned>/bin/unicd`. All CLI commands inherit the same private environment. Sources, exports and binary paths are task-owned. Pairing tickets and fingerprints are never published.

```text
unic device addr
unic device add <private-test-address>
unic device approve <test-peer-id> <independently-matched-fingerprint>
unic send --to all --json --request-id <case-key> -f <fixture-root> [<other-root> <loose-file>]
unic recv list
unic history export <received-history-id> --output <owned-export-directory>
unic history delete <received-history-id> --yes
unic config set limits.max_file_bytes 1
unic config set limits.max_file_bytes "100MB"
unic device remove <test-peer-id>
unic dev submission <case-key>
```

Text uses `unic send --to all --json --request-id <case-key>` with deterministic fixture bytes on stdin. Cancellation invokes the production `DELETE /v1/submissions/<case-key>` API after observed scanning/preparing or receiver payload state. Status, connection, operation and generation-stop control use the existing production local API. After a CLI timeout, the harness observes the original operation ID; it does not submit again. Export/hash verification remains mandatory.

The real harness builds native workers and production binaries with `CGO_ENABLED=0 GOOS=<target> GOARCH=<target> GOFLAGS= GOWORK=off go build -mod=readonly -trimpath -buildvcs=false`. The process runner separately uses `-race -tags unic_e2e`; its receipt records every build/test argument. These binaries are distinct evidence layers.

## Final corrected-code commands

```sh
.cache/f-directory/unic-e2e-final-io run \
  --lab e2e/real/directory.functional.local.toml --out .cache/f-directory/functional-final
.cache/f-directory/unic-e2e-final-io run \
  --lab e2e/real/directory.performance.final.local.toml --out .cache/f-directory/performance-final
python3 tools/directorye2e/run.py --output .cache/f-directory/process-final-io
UNIC_HEADLESS_E2E=1 UNIC_HEADLESS_ARTIFACTS="$PWD/.cache/0015-acceptance-final-io" \
  go test -race ./e2e/headless -run '^TestProductionHeadless$' -v -count=1
```

The functional inventory selects only `directory` and enables recipe `[0]` for validation; the scenario itself creates its active 16 MiB fixtures. It executes 32 checks, including one intentional owned-service crash per node. The performance inventory selects only `pairwise` and recipes `[0, 1024, 16777216]`, producing six directed measurements. Source cancel calls have a separately asserted 5 s local API/CLI completion budget. The active-fault `milliseconds` field includes the negative receive-observation window, resource polling and any restart/reconnection; it is an end-to-end observed upper bound, not the isolated cancel API duration. Historical receipts do not persist that isolated API duration; they retain its passing budget assertion. Descriptor counts on macOS remain unavailable (`-1`).
