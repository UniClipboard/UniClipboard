# Directory CLI delivery: macOS and Linux

Date: 2026-11-19 (Asia/Shanghai). Branch: `feat/directory-transfer-cli`. Starting HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. Producer implementation commit: `278fe6da1` (full value in [provenance](provenance.json)). The later harness-only reporting fix corrects directory recipe byte budgets. No push, PR, merge, release or production deployment occurred.

## Final results

| Layer | Result | Evidence |
|---|---|---|
| Real macOS arm64 / Linux amd64 functional catalog | PASS: 32 checks, both directions, including active cancellation, receiver interruption, revocation, source crash, fresh recovery and user-owned export preservation | [Functional report](final-functional/report.md), [manifest](final-functional/manifest.json.gz) |
| Real-host semantic / 1,024-small-file / 16 MiB directory catalog | PASS: 6/6 directed cells; every root name, relative path, directory, file size and SHA-256 matched after production CLI export | [Performance report](final-performance/report.md), [raw measurements](performance.json) |
| Frozen race CLI matrix | PASS: 20 scenarios, including fixture-board repeated copy, automatic no-echo, newest-copy protection and ordinary-content regressions | [Process receipt](final-process/receipt.json) |
| Production headless lifecycle/content/UI-dependency boundary | PASS | [Headless manifest](final-headless/manifest.json.gz) |
| Independent exact-path cleanup | PASS for both final real-host runs | [Functional cleanup](cleanup-functional-body30.json), [performance cleanup](cleanup-performance-body30.json) |
| Native system clipboard copy and cross-application directory paste | NOT RUN | No GUI, browser, VM or personal clipboard was used |

Production binaries are identical between the final functional and performance runs. All 532 frozen repository Go files checked outside the harness match the committed files, including producer inputs. Module files match. Each receipt preserves its actual initial HEAD and frozen working-tree hashes; later documentation markers do not retroactively change those identities. See [provenance](provenance.json).

## Product corrections

1. `unic history export` reconstructs snapshots under `rN/<original-root-name>`. It preserves equal-name roots, nested/empty/hidden/zero-byte entries and Unicode names. It verifies file sizes/hashes, rejects existing containers, rolls back only newly created containers and keeps successful copies independent of history cleanup. Directory references require an explicit snapshot first.
2. A cancelled/disconnected sender could leave an unpublished receive file waiting indefinitely for stream EOF. Built-in sessions now bound each blocked body read/write with a separate 30 s inactivity deadline. Continuous transfers may last longer. Failed/aborted receives end the session instead of draining an unfinished rejected body. Idle peer header waits retain their existing behavior.

The final functional catalog includes one intentional task-owned service crash on each node. Cleanup required no forced-stop fallback. Both profiles, SSH bootstrap and controller private directories were removed. Linux staging/fixture descriptor counters reached zero. macOS descriptor counts are unavailable (`-1`); its process/socket/path cleanup and directory-view ownership were verified separately. A deferred fixture-board directory view was checked only after its remaining Received holder was explicitly dismissed through the production CLI. Published custom receive files and successful CLI exports survived history deletion/restart.

## Timing and budgets

The semantic fixture has two equal-name directory roots plus a loose file, including empty directories and a zero-byte file. The small-file fixture has 1,024 files of 1 KiB. The large-file fixture has one 16 MiB file. One repetition is exploratory, not a percentile or stability claim. [Machine conditions](environment-linux.json) and [local conditions](environment-mac.json) describe shared hosts; network route is unknown.

| Direction | Input | Preparation + admission ms | Send operation ms | Complete export/hash verification ms |
|---|---|---:|---:|---:|
| Linux to macOS | semantic 226 file bytes | 45.90 | 5 | 582.34 |
| macOS to Linux | semantic 226 file bytes | 28.18 | 1 | 556.98 |
| Linux to macOS | 1,024 x 1 KiB | 1128.22 | 2840 | 39153.06 |
| macOS to Linux | 1,024 x 1 KiB | 542.14 | 700 | 51119.65 |
| Linux to macOS | 16 MiB | 246.94 | 95162 | 99849.95 |
| macOS to Linux | 16 MiB | 64.22 | 58529 | 80019.83 |

All six final cells met the original 60 s preparation/admission, 180 s complete-verification and 512 MiB sampled service RSS budgets. Maximum sampled service RSS was 71.69 MiB. Preparation/admission includes CLI startup, import, snapshot preparation and processors. Send-operation timing includes queue, transport and acknowledgement. Complete verification also includes SSH/control, receiver export, per-file fsync and hashes. Throughput in the JSON uses file bytes per send-operation second, not pure wire throughput. Exact internal scan spans and OS/allocator high-water marks were not measured.

The Linux-to-macOS 16 MiB CLI initially exited 4 with `timeout` after its existing 60 s observation window. The persistent service finished that same operation, and receiver export/hashes passed within the original case budget. It was never resubmitted. The other final cells initially exited 0. This is eventual transfer acceptance; it does not turn the initial timeout into a successful CLI exit.

The source cancellation API/owned-CLI completion retains its original 5 s assertion. Active-fault elapsed values include the two-second negative receive window, resource polling and recovery; they are observed end-to-end upper bounds. Historical receipts did not persist the isolated source API duration, only its passing assertion. The initial 5 s transient-resource check failed. The final resource observation gate is 45 s, selected after diagnosis around the 30 s body inactivity policy; it is not an original preset performance gate. See the [failure inventory](../../0024-cli-failure-inventory.md).

## Retained failures and limitations

Earlier failures remain failed. The [pre-fix cancellation receipt](failed-cancel-before-fix/report.md) records an unpublished 807,672 B file remaining at the macOS receiver. A [five-second body-I/O experiment](failed-five-second-io/report.md) rejected a 16 MiB transfer after 4,050,524 bytes. The 5 s control-event budget was therefore not reused for bulk bodies. [Earlier combined performance/fault evidence](performance-and-fault-failure/report.md), [initial failed large-send evidence](failed-full-1/report.md) and [intermediate harness failures](intermediate-failures.json) preserve their own outcomes and cleanup. Successful reruns do not relabel them.

Historical controller `planned_payload_bytes_upper_bound` values misinterpreted directory recipe numbers as total bytes. Runtime inventories, actual byte measurements and the independent 1 GiB free-space admission check remained correct. [The corrected CLI plan](corrected-plan.json) includes all file bytes and a bounded manifest allowance. The original planning numbers remain unchanged in the historical receipts.

Native clipboard/paste, Windows/mobile runtime, exhaustive filesystem failures, abrupt host power loss, packet routing changes and exact OS handle counts on macOS remain outside this CLI acceptance. The fixture-board receive race proves the production adapter recheck with synthetic clipboard state, not OS paste. Service EOF/process/socket cleanup does not establish every possible native clipboard holder lifetime.

## Replay and verify

From the repository root, use an already authorized SSH alias and a fresh output directory:

```sh
docs/exec/baselines/directory-cli-0024-2026-11-19/replay.sh primaryserver /absolute/new/receipt
```

The script runs frozen race CLI acceptance, the 32-check real-host functional suite, then the independent six-cell performance suite and production headless acceptance. Fresh identities and synthetic data are used. Source/worker/producer hashes are checked. Owned profiles, source/build copies and raw private logs are removed on success/failure. No personal history is read. See [exact commands](commands.md).

Check every retained file from this receipt directory with `shasum -a 256 -c SHA256SUMS`. JSON manifests/results use deterministic gzip compression; inspect them with `gzip -dc final-functional/manifest.json.gz`. Retained artifacts contain hashes and small receipts, not source snapshots, databases, test keys, executable binaries, raw logs or recordings.

## PR integration

After the original local delivery, the branch merged the latest `v2` at `350ac4ce1`. Only two architecture `verified` markers conflicted. Directory export/session producer modules are unchanged. The merged source passed all 20 frozen race CLI cases, production headless acceptance and documentation checks. See [the integration receipt](pr-integration.json), [input inventory](pr-integration-source-files.json.gz) and [headless manifest](pr-integration-headless.json.gz). Original physical-host receipts remain bound to their recorded pre-integration source; they are not reissued for this merge. Statements about no publication above describe the initial delivery checkpoint.
