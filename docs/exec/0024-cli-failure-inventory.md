# Directory CLI acceptance inventory

This inventory precedes the new acceptance implementation. It supplements [0024](0024-directory-copy-and-send.md). Production CLI export is distinct from native clipboard paste.

## Failure modes and assertions

| Scenario | Failure to detect | Required oracle |
|---|---|---|
| Bidirectional minimal tree | Missing/flattened roots, lost empty directories, corrupted bytes | Compare root names, every relative path, directory flags, sizes and SHA-256 after production CLI export on both hosts |
| Mixed/equal-name roots | Root collision or descendant overwrite | Preserve ordered distinct root containers and complete inventories |
| Repeated send/copy | Duplicate admission, undetected fresh copy or echo | Independent requests; copy-generation checks remain fixture-adapter evidence |
| Default-off/opt-in | Recursive work while disabled, source edits initiating sends | Existing scan/snapshot counters and bounded receive assertions |
| Cancellation | Late publication, partial export, retained readers/staging | Cancel scan/preparation/send/receive separately; verify terminal state and staging/resource release |
| Competing local copy | Old receive replaces newer selection | Existing deterministic fixture-adapter race; native clipboard NOT RUN |
| Source change/deletion | Silent partial snapshot or stale success | Stable error, no partial publication; preserve source |
| Offline/restart/interruption | False success, replay, leaked process or staging | Observe connection state, stop only owned generation, restart and verify fresh transfer |
| Revocation/bounds | Unauthorized transfer or bypassed file/depth/entry limits | Reject batch; no partial Received/history; surviving fresh transfer |
| Export failure/ownership | Overwrite destination or delete user output | Existing destination rejection, rollback only newly created outputs, retained exported files after history deletion/restart |
| Resource release | Retained leases, staging, handles or children | Observe owned paths, process exit/socket close, successful follow-up; distinguish indirect evidence from measured handle counts |

## Performance plan fixed before measurement

Use deterministic synthetic trees: semantic small tree; 1,024 files of 1 KiB with nested/empty directories; one 16 MiB file. Both directions use the same recipe. File bytes stay below existing 100 MB default and entry count stays below 4,096. Disk budget: at most 512 MiB per node for these fixtures plus binaries; do not fill a filesystem. Minimum free disk: 1 GiB. One sample is exploratory and does not establish percentiles.

Set a 180 s receive deadline per case, 60 s preparation budget, 512 MiB service peak RSS budget, 5 s cancellation budget, and 5 s newer-text progress budget. These are conservative acceptance budgets for isolated synthetic workloads on shared hosts, not throughput promises. Record observed values and mark any unavailable stage/handle measurement NOT RUN. Do not infer preparation/transport separation from a combined submit timer. Network route remains unknown unless observed. Controller verification includes SSH/export costs. Do not choose thresholds after results.

## Evidence policy

Freeze source; verify native binary hashes and salted host identities. Use only owned profiles, synthetic data and production content paths. Retain small JSON receipts, exact replay parameters and SHA256SUMS. Remove raw logs, test identities, source copies, binaries and profiles. A required host timeout is FAIL. Untested scenarios remain NOT RUN. No GUI or system clipboard is authorized for this CLI task.

The real-host `directory` catalog uses worker-local polling of the production submission stage before cancelling scanning/preparation or mutating an unread member. Missing the required stage is a failed test, never a substituted success. Mid-transfer cancellation, receiver stop and revocation require observed payload bytes in the receiver's private staging directory before injection. These operations affect only task-owned services. Concurrent text uses a separate explicit CLI operation while directory preparation is observed; it does not claim clipboard supersession.

A persistent production service may continue the same admitted send after the CLI's existing 60 s observation window returns exit 4 with outcome `timeout`. The harness records that initial result. It may observe the same operation to a terminal result within the original 180 s case budget, then independently export and verify the receiver. It never retries or resubmits to convert a failure. A terminal operation error, missing receive, or expired case budget remains FAIL. This is eventual transfer acceptance, not a claim that the initial CLI invocation exited successfully.

The follow-up `sender-crash` case kills only the worker's observed owned service process after receiver bytes exist. It waits for the CLI child to fail, verifies receiver staging release without relying on a graceful TCP FIN, restarts the same isolated profile, checks transient cleanup and verifies fresh transfer. Intentional crashes are counted separately from forced cleanup fallbacks. This specifically exercises the stalled-body lifetime failure mode already exposed by the original real-host cancellation E2E.

## Diagnosed body-lifetime policy and revised observation gate

The initial five-second transient-resource check failed on real hosts. A five-second blocked-body I/O experiment then rejected a real 16 MiB transfer after approximately 4 MiB. Small control events and bulk bodies cannot share that timeout policy. Production body reads and writes now use a separate 30-second inactivity deadline. It refreshes per underlying I/O call and does not limit total continuous transfer time. Receive errors/aborts close the session rather than draining an incomplete rejected batch.

The follow-up resource-observation gate is 45 seconds. This is an engineering bound selected after diagnosis to cover the 30-second body idle limit, in-flight progress and control observations. It is **not** an original predeclared performance gate. Earlier failed five-second checks remain FAIL. The original 5-second source cancellation API/CLI budget, 60-second preparation/admission budget, 180-second performance case budget and 512 MiB sampled service RSS budget remain unchanged. Active-fault elapsed observations include the two-second negative receive window and any recovery; they do not represent isolated source API latency. The exact isolated API latency was not retained in historical receipts; its 5-second assertion is retained.
