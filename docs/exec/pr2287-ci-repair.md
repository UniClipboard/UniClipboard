# PR 2287 CI repair

## Failure and contract

The `Test and build` job in [run 44986452348](https://github.com/UniClipboard/UniClipboard/actions/runs/44986452348/job/137097966203) failed at `engine_test.go:519` on source `195e0edc332ca5a055a4a69d85fd62bc81237d6d`.
`TestPairByTicketNeedsApprovalOnBothSides` required an empty device list immediately after unpairing. The observed device was pending at revision 4 with no bindings.

The [identity architecture](../architecture/identity-and-trust.md) permits a verified handshake to return a revoked device to pending. The [device contract](../contracts/device-contract.md#5-权限与撤销) requires revocation of trust and path authorization, followed by new approval. An empty list is not stable while the other device is running.

## Reproduction and change

The unchanged test passed 30 local race-enabled runs. The original CI failure remains the first failure evidence; those passes do not explain away the failure.
Adding an explicit fresh ticket handshake after unpairing made the old empty-list assertion fail deterministically: the same device returned at revision 4, with pending trust and a pending binding.
This controls the return-to-pending state; it does not reproduce the exact scheduling of the original background handshake.

The existing test now checks the authorization invariant immediately after unpairing and after the explicit handshake. A returned device must have pending trust, a newer revision, and only pending bindings. Both connections must close, and the fresh handshake must not restore a connection without approval. Production behavior is unchanged.

## Repeatable validation

```sh
TMPDIR=/private/tmp go test -race ./engine -run '^TestPairByTicketNeedsApprovalOnBothSides$' -count=30 -timeout=3m
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_ARTIFACTS=/tmp/pr2287-e2e go test ./e2e/cli -run '^TestSyncPluginWorkflow/singleton$' -count=1 -v -timeout=4m
(cd /tmp/pr2287-e2e && shasum -a 256 -c SHA256SUMS)
```

`TMPDIR=/private/tmp go test -race ./...` passed. `go run ./tools/docscheck` passed with six existing stale-page warnings. `git diff --check` passed. The production `unic`, `unicd` and `unic-desktop` builds passed. The corrected test passed all 30 runs. The existing real CLI/service/external-plugin E2E passed in 16.31 seconds, excluding binary construction. It checked bidirectional text, PNG and files, policy enforcement, revocation, restart without restored trust, backend disablement and child cleanup. It uses private loopback transport and no native UI or system clipboard.

The local JSON receipt is `/tmp/pr2287-e2e/sync-plugin-workflow-singleton.json`. Its SHA-256 is `0f22c154b5c4fac19015a2775d321a6459f53653461e7e33eda3ebe0faf1c80e`. It contains program hashes and passed steps. To regenerate `SHA256SUMS` after another run, use `shasum -a 256 sync-plugin-workflow-singleton.json > SHA256SUMS` in the artifact directory. First-failure and local run logs remain temporary files under `/tmp/pr2287-*`.

## Desktop translation failure

On commit `10a37c598`, `Test and build`, CLI construction and five Desktop shards passed. [Desktop shard 3](https://github.com/UniClipboard/UniClipboard/actions/runs/44987092513/job/137099997826) failed the `i18n` case because `action_result.go` used an uncatalogued source string: `已自动复制到剪贴板`. The failure artifact is `desktop-e2e-evidence-3` in that run. The first local canonical run, `20261122T050740Z-d7c69918`, reproduced the same missing key. No retry was used to dismiss the failure.

The new Action result dialog and status notifications introduced 14 source strings without catalogue entries. All 14 now have English, Japanese, Brazilian Portuguese and Russian translations. Format placeholders remain unchanged. This completes the existing [Desktop language architecture](../architecture/overview.md#desktop-languages).

`make desktop-e2e CASE=i18n` passed after the catalogue change. The canonical software-view report is `.build/desktop-e2e/20261122T050825Z-8da029e8/index.html`. Its `SHA256SUMS` verification passed. The report covers five languages and both themes; it does not establish native OS behavior. Repeat the command to regenerate the report and verify its checksums from its output directory. The first failed report remains available separately.
