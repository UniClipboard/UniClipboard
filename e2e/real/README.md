# Real-machine CLI harness

Run one test suite against any configured set of macOS, Windows and Linux hosts. The controller uses local processes or OpenSSH to deploy a native Go worker. Workers execute production `unic` and `unicd` in private profiles. SSH carries control and deployment traffic; clipboard content travels through real unic/tailcat connections.

See the [three-machine acceptance and retained failures](../../docs/exec/real-machine-harness.md) for executed coverage.

This is the entry point for new physical-host testing. `e2e/twohost` preserves the earlier fixed-host scripts and receipts.

## Requirements

The controller needs Git, Go compatible with the root module, and OpenSSH `ssh`/`scp` on PATH. The controller runs from the repository root, or supplies `--source /path/to/repository`.

Remote hosts need a reachable SSH server, noninteractive key authentication and executable temporary storage. They do not need Go, Python, a GUI, a desktop login session or a globally installed unic. Windows requires PowerShell, AF_UNIX support and the default OpenSSH command shell (`cmd.exe`). Custom SSH command shells require separate bootstrap acceptance. Unix hosts need `uname`, `mktemp`, `chmod`, `df` and a machine identity: `/etc/machine-id` on Linux or `ioreg` on macOS. Supported targets are macOS/Linux/Windows amd64 and arm64. A controller on each operating system is supported by the code; consult the acceptance receipt for platforms actually exercised.

First verify each SSH alias yourself. The harness honors your OpenSSH configuration, including users, keys, known hosts and jump hosts. It does not disable host-key checking, install services or change firewall settings.

## Configure your machines

```sh
go build -o .build/unic-e2e ./e2e/real
cp e2e/real/lab.example.toml e2e/real/lab.local.toml
```

Edit the local inventory. `*.local.toml` is ignored in this directory.

```toml
version = 1

[nodes.a]
transport = "local"

[nodes.b]
transport = "ssh"
host = "test-win"

[nodes.c]
transport = "ssh"
host = "test-linux"

[suite]
topology = "mesh"
repetitions = 3
receive_timeout_seconds = 180
scenarios = ["contract", "pairwise", "mesh", "isolation"]
text_bytes = [1, 1024, 65536, 1048576]
png_pixels = [16, 256, 1024]
file_bytes = [0, 1024, 1048576, 16777216, 104857600]
html_bytes = [1024, 65536, 1048576]
```

Node names are arbitrary labels, not operating-system roles. Use 1–32 ASCII letters, digits, underscores or hyphens, starting with a letter. A local node cannot have `host`. An SSH node requires a host or alias accepted by OpenSSH. Accounts, keys and jump-host details stay in your SSH configuration. The controller need not be a test node: all nodes can use SSH.

Add another node table to add another machine. No scenario source changes are required. Unknown configuration fields, invalid sizes and duplicate scenario selections fail validation.

Sizes are bytes. PNG sizes are square image edge lengths in pixels. Text supports 1 B–64 MiB, HTML plain-text components 1 B–16 MiB, files 0 B–100 MiB, and PNG edges 1–2048. The HTML fixture includes both the plain text and a valid HTML wrapper. `receive_timeout_seconds` bounds each receiver wait. It defaults to 180 seconds and accepts 1–900. Choose a larger value for slow WAN links; a timeout remains a failed target and is not discarded. An explicit empty array disables that content type. At least one payload must remain enabled. The default catalog is the one in `lab.example.toml`.

For a short first run, use one repetition, text `[16]`, PNG `[16]`, files `[0, 1024]` and HTML `[1024]`. Use more repetitions for performance work. Three samples are insufficient for a reliable tail percentile.

## Run

```sh
.build/unic-e2e plan --lab e2e/real/lab.local.toml
.build/unic-e2e doctor --lab e2e/real/lab.local.toml
.build/unic-e2e run --lab e2e/real/lab.local.toml --suite sync
```

`plan` does not contact hosts. It shows pairing edges, directed links, payload cells and repetitions.

`doctor` freezes source, builds workers, probes platforms and machine identities, checks temporary storage and verifies worker hashes. Two aliases resolving to the same OS machine identity fail by default. These identities distinguish configured OS installations; they do not attest bare-metal hardware. Machine identifiers are salted per run before entering public evidence.

`run` also builds production programs from that same snapshot, copies the correct target binaries, verifies hashes on each host, installs the existing Lua processor fixtures and executes the selected scenarios. Builds use `-trimpath`, `-buildvcs=false`, `-mod=readonly` and `CGO_ENABLED=0`. The manifest records the actual toolchain and complete program hashes. Source files are copied before any build; subsequent edits in the developer checkout cannot change the run. Repository-internal regular-file symlinks are materialized and recorded; links outside the repository are rejected.

The output defaults to `.cache/real/<run-id>`. `--out DIR` selects another directory, which must not exist. Input validation errors happen before a run directory exists. Once execution begins, failures retain a failed manifest and report.

## Scenarios

Scenario selection is a set. Execution order is contract, pairwise, mesh, then isolation. The controlled catalog enables the sensitive and email-tag Lua examples on every node, including timing rows. Latency includes those processors and their configured budgets; it is not a builtin-only production baseline.

- `contract`: no-target local commit; same-key query/retry and conflicting-input rejection; matching pairing fingerprints and both approvals; no pre-approval delivery; four concurrent CLI retries with one delivery; local processor tags; sensitive-content suppression and expiry; global/type filters and no replay after enable.
- `pairwise`: start only each selected pair of services, approve that pair and measure both directions. Other workers remain available, but their services are stopped. For N hosts there are N(N−1)/2 pairs and N(N−1) directed links.
- `mesh`: start and mutually pair all nodes. Each sender submits once per payload/repetition. Every other node must receive exactly one matching record. Each receiver contributes its own sample. Samples share a case ID and a last-receiver latency estimate. For exactly two nodes, mesh also supplies the isolated pairwise baseline instead of repeating identical work.
- `isolation`: seed durable history, stop/restart all services, assert unchanged device identities/trust/history without repairing lost trust, stop one node, verify surviving-node transfers, recover without offline replay, transfer new content, and revoke one relationship while other nodes remain usable.

Pairing always compares the two independently returned fingerprints before either approval. Introductions do not substitute for approval and do not forward content.

Fixture generation and SSH deployment finish before timing starts. Text/PNG/files use production CLI submission. HTML uses a worker-side test injector through the production local API because `unic submit` has no HTML flag. Every receiver exports through the production CLI. All expected representations, MIME types, sizes and SHA256 values must match. A `queued` or `sent` result alone never passes a transfer. Receive count audits use bounded observation windows.

## Timing

Workers exchange twelve native timestamps over their persistent control connections before and after each payload cell. The shortest round trip bounds each node's offset uncertainty. A sample from one remote host to another combines both uncertainties, 1 ms timestamp quantization and the observed offset drift between calibrations.

`receive_estimate_ms` measures sender-local submitting-client start through the receiver Engine's completed receive timestamp. It includes startup, local IPC, commit, transport and storage. It excludes controller export round trips. `verified_ms` uses the controller's monotonic clock and also includes SSH/control, polling, export and hash verification. `submit_ms` is controller-observed submission-call duration. None is labeled pure network latency.

Raw estimates and lower/upper bounds are retained. Bounds touching zero do not resolve precise one-way latency. Wide-area SSH calibration can have a large uncertainty; the report retains it. Network path is `unknown` unless independently observed. Management connectivity does not establish unic direct/DERP routing.

## Artifacts and cleanup

Each run directory has two parts:

- `public/`: `manifest.json`, `results.jsonl`, `outcomes.jsonl`, `report.md`, and `SHA256SUMS`. These contain source/program hashes, run-scoped machine identity hashes, expected and received part hashes, checks, clock bounds, raw timing samples and cleanup results. Per-target outcomes retain completed content checks and failures even if another receiver times out. A failed fanout does not produce a successful all-target latency sample.
- `private/`: temporary frozen source, built programs, SSH diagnostic output and node workspace/connection inventory. The harness removes it by default after node cleanup and snapshot verification. Use `--keep-private` to retain it for diagnosis. Do not publish it.

The worker owns one service and private profile. It stops the observed service generation through the existing local service API after verifying the PID. Unix SIGTERM or Windows termination are fallbacks and forced stops are recorded. After process exit it checks that the private service socket is unavailable. It reads captured daemon stdout/stderr only after the service exits, checks known ticket/content sentinels there and never copies raw logs into public reports. Persistent structured logs under the private profile are not included in this sentinel scan; this is not complete product-log privacy acceptance. Both successful and failed runs remove node profiles and remote bootstrap directories after verifying their ownership marker. The controller verifies the source snapshot before removing its own `private/` directory. Public reports, measurements and checksums remain.

Control EOF cancels work and stops the owned service. An idle lease bounds abandoned workers; the controller renews it during long scenarios. Cleanup failures make the run fail. `--keep-private` explicitly retains stopped node profiles, remote bootstrap programs and controller private artifacts on either success or failure. The manifest records `keep_private` and `controller_private_removed`. If node cleanup cannot be confirmed, the controller retains its private recovery inventory and reports cleanup errors. A lost network connection, forced controller termination or power loss can prevent complete cleanup; those cases do not establish successful cleanup. Each run owns only its own artifacts and does not sweep historical run directories. No global `pkill`, service installation, user profile changes or system clipboard access is used.

`--allow-shared-hosts` is an explicit logical-node control mode. Its manifest still records the smaller distinct-machine count and marks the run as a shared-host control. Do not use it as physical-machine evidence.

A CI executor can use the same commands when it has the configured SSH credentials and network access. An unreachable required node is a failure, not a skipped test.

## Boundaries

This harness exercises headless CLI/service behavior. It does not launch desktop applications or establish native clipboard, rich-text rendering, sleep/wake or network-switch acceptance. Abrupt power loss and idle crash-detection latency are not part of the cooperative service-stop scenario. Concurrent senders and additional graph topologies are not supplied by this first catalog. Android/iOS require future node adapters; they are not SSH desktop targets.

## Harness process acceptance

To retain diagnostic artifacts explicitly:

```sh
.build/unic-e2e run --lab e2e/real/lab.local.toml --keep-private
```

The gated process test executes compiled harness and production programs. It verifies a configured one-second receive timeout, doctor cleanup, control-EOF cleanup against the actual service PID/socket, default success/failure private cleanup, explicit diagnostic retention, duplicate-machine rejection with a failed receipt, and four-node plan expansion without network execution. It is an E2E process test, not a unit test or a substitute for the multi-host run.

```sh
UNIC_REAL_E2E=1 \
UNIC_REAL_E2E_ARTIFACTS="$PWD/.cache/real-harness/process-acceptance" \
  go test -race ./e2e/real -run '^TestHarnessProcesses$' -v -count=1
```

Use a fresh artifact directory. The process test retains its own summary `manifest.json` plus private build/setup directories. Publish the summary and the nested doctor public receipt, not the entire directory.
