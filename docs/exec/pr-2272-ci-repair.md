# PR 2272 Desktop CI bootstrap repair

## Failure inventory and acceptance boundary

Written before the workflow change. The acceptance boundary is the existing
three-shard hosted Desktop E2E workflow. Do not launch a local desktop, VM or
system clipboard to reproduce a hosted Ubuntu package-download failure.

| Failure | Observable assertion |
|---|---|
| Dependency downloads consume the entire job | Package bootstrap must finish inside a five-minute step budget. The job retains its thirty-minute limit. |
| A regional archive stalls a package connection | Use the official Ubuntu HTTPS archive for EC2 archive entries and finite APT connection/data timeouts with bounded retries. Preserve signature verification. |
| Mirror rewriting targets the wrong distribution file | Operate on the Ubuntu 24.04 deb822 source file used by the pinned hosted image; retain all suites, components and signing configuration. A missing file fails setup. |
| Runner mirror-list indirection bypasses the chosen archive | When `/etc/apt/apt-mirrors.txt` exists, replace its candidates with the official HTTPS archive before updating package indices. Verify the hosted download log uses that destination. |
| A test failure is hidden by infrastructure changes | Run all three original shards with their existing cases, assertions, recordings and artifact upload. Require all three checks to pass on the pushed PR head. |
| Increasing timeout hides missing execution budget | Keep the job at thirty minutes and preserve the catalogue partition and race instrumentation. |
| An old passing run is mistaken for the repair | Bind the new workflow run and every job result to the exact pushed source SHA. |

## Original failure

- PR head: `13aa63ce7cace0bade33d606ab35f5246a037621`.
- Workflow: [44486470052](https://github.com/UniClipboard/UniClipboard/actions/runs/44486470052).
- Shard 2 job: `135431069046`; shard 3 job: `135431069215`.
- Both job annotations report the thirty-minute execution limit.
- Shard 2 dependency setup ran from 08:18:38 to 08:39:30 UTC. APT downloaded
  98.8 MB in 20 minutes 20 seconds from a regional EC2 Ubuntu archive.
- Shard 3 setup ran from 08:18:44 to 08:42:27 UTC. APT downloaded 105 MB in
  22 minutes 57 seconds. Its log shows stalled package retries against the same
  archive family.
- Both shards continued producing passing case results until cancellation.
  No assertion failure was reported before the budget expired.
- Shard 1 completed the same dependency setup in approximately three minutes,
  then passed its full case selection.

The original hosted workflow is the red end-to-end result. Its complete logs and
job annotations distinguish bootstrap starvation from a test deadlock or external
manual cancellation. The repair is validated by another full hosted run; local
syntax checks cannot establish Ubuntu download or native-window acceptance.

## Repair

The first repair run, [44492010370](https://github.com/UniClipboard/UniClipboard/actions/runs/44492010370),
exposed runner mirror-list indirection. Direct deb822 URI rewriting did not affect
`file:/etc/apt/apt-mirrors.txt`. Two installation steps reached the new five-minute
limit while still downloading from regional EC2 mirrors. The corrected workflow
also configures that existing mirror list; it does not extend either timeout.

The PR Desktop jobs rewrite only regional EC2 Ubuntu archive URIs to
`https://archive.ubuntu.com/ubuntu` in the pinned Ubuntu 24.04 deb822 source file.
The package set, suites, components and signing configuration remain unchanged.
Both APT commands use three retries and thirty-second HTTP/HTTPS connection/data
timeouts. Package setup has a five-minute step limit. The complete three-shard
canonical suite retains its thirty-minute job limit and all assertions.

Local `actionlint` and the extracted installation step's `bash -n` check pass.
These checks establish workflow/shell validity, not hosted installation success.
The acceptance result is the complete replacement PR-check workflow for the
pushed repair commit. Inspect [PR 2272 checks](https://github.com/UniClipboard/UniClipboard/pull/2272/checks)
for the exact source SHA, job results and canonical recordings. The local
sanitized follow-up receipt is written under `.cache/ci-2272` after that run.
No local GUI, VM or system clipboard is used. No test assertion or timeout is
relaxed to obtain a passing result.

APT's official [HTTP transport documentation](https://manpages.debian.org/unstable/apt/apt-transport-http.1.en.html)
defines the connection/data timeout. The [HTTPS transport documentation](https://manpages.debian.org/unstable/apt/apt-transport-https.1.en.html)
uses the corresponding HTTPS scope. The [APT configuration reference](https://manpages.debian.org/trixie/apt/apt.conf.5.en.html)
defines the finite retry count.
