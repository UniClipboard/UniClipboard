# External sync plugin workflow

## Scope and identity

Worktree: `7d156c15-73dc-4c64-a305-83836e301bd7`.
Branch: `feat/sync-plugin-workflow`.
Base: `origin/v2` at `894790a3a4dc512ab622b0cf9120895251955173`.
`origin/HEAD` points to `origin/main`; this task never used it as its base.

The user approved the implementation/instance identity resolution after the first single-instance checkpoint (`1c7256c`, documented by `79a3958`). Installation and handshake `Spec.ID` identify the program. `PluginInstance.ID`, `backend.bind.backend_id`, policies, directories, settings, operations and bindings identify the local runtime instance. The receiver explicitly selects a compatible local instance. RPC frames, identity proofs, peer metadata and ticket envelopes use version 1. No compatibility branches or new object-store/login protocol are introduced.

## Delivered flow

1. Install a Go sync implementation through the existing GitHub installer.
2. Create a disabled runtime instance. It has independent settings and private data.
3. Configure Schema values and secrets. Public responses omit secrets and report presence booleans.
4. Enable the instance, set its standard policy and restart. The service reports restart requirements and local runtime availability.
5. Select a local peer-sessions instance when exporting or joining a pairing code/ticket. Remote instance IDs may differ. Unknown, unavailable, incompatible and wrongly addressed issuer instances fail without fallback.
6. Compare the fingerprint and approve each side's exact device/binding revision set. A trusted device still needs approval for a new path.
7. Send and receive content. History retains separate backend outcomes. Two same-view deliveries commit/apply once; one backend failure or child crash does not erase another backend's success.
8. Pause, resume or remove one path using CAS. Whole-device removal revokes every path. Restart does not replay uncertain content.

Tailcat can be disabled. Zero backends preserve capture, history and local Actions. Standard policies apply per running instance; global sync disables content work across all of them. Assembly and Go settings changes require restart. On a changed Go configuration, Engine atomically revokes that instance's old device bindings and external channels before starting its new process. Device trust and other instances remain intact. Because Schema does not identify account-context fields, this conservative invalidation applies to every Go backend configuration change. Program identity is sticky; reassigning an instance to another implementation is rejected.

## CLI and Desktop

```sh
unic plugin install https://github.com/OWNER/REPO
unic plugin instance add IMPLEMENTATION INSTANCE
unic plugin settings show INSTANCE
unic plugin settings save INSTANCE settings.json
unic plugin enable INSTANCE
# Restart unicd after assembly or Go settings changes.
unic device backends
unic device addr --backend INSTANCE
unic device add CODE_OR_TICKET --backend LOCAL_INSTANCE
unic device list
unic device approve DEVICE_ID 'FINGERPRINT_SHOWN_BY_OTHER_DEVICE'
unic config set 'plugins.instances."INSTANCE".sync.text' false
unic device list --json
unic device path pause BINDING_ID REVISION
unic device path resume BINDING_ID CURRENT_REVISION
unic device path remove BINDING_ID CURRENT_REVISION
```

Desktop: Settings > Plugins > installed Go backend > create instance; open the new instance to configure/enable it, then restart. Invite and Connect select a running compatible instance. Approval names the local paths. Device details show per-path controls. History detail shows per-instance results. The installed implementation is not duplicated on disk for each runtime instance.

## Capability boundaries

- `peer_sessions`: codes/tickets, Engine identity proof, device trust, per-instance approval and content transfer are implemented.
- External-channel profile: implementation aliases, independent group targets and receives work through the existing SDK/RPC profile. They create no device/fingerprint and expose no device-pairing capability. Its evidence is a real child-process SDK E2E, not third-party login acceptance.
- Object-store device/member protocols, WebDAV/S3 object formats and third-party account/login flows remain unimplemented. Configuration disclosure is not a claim that those transports work. No protocol was invented for this task.
- Native acceptance uses private profiles and a directory clipboard. OS clipboard, physical input, cross-application paste, other-platform native acceptance and user visual approval are separate.

## Failure inventory and retry discipline

Before implementation, the task-local plan listed: plugin/capability misclassification; Tailcat fallback; missing programs; shared instance processes/data/settings/locks; wrong-instance tickets; trust inherited by a new binding; expired or changed proofs/approvals; duplicate device rows; per-instance/global policy bypass; explicit-send privilege escalation; cross-backend result corruption/deduplication; uncertain replay; surviving processes/resources; secret disclosure; and non-P2P backends forced through device pairing.

Retain the first failure. Allow at most two bounded diagnostic reruns for one identified failure; compare recurring failures and do not resend content until it happens to pass. Changed cases and distinct causes retain separate identities. Relevant unexplained failures block delivery.

Observed and resolved:

- The initial local-mode test exposed the Tailcat-disable configuration rejection.
- The original fixture used an obsolete SDK entry point; the first join harness used `address` rather than `addr`; a file fixture omitted its required name; the Schema fixture initially omitted its profile-version key; and the instance-settings harness initially asserted markers without saving its settings. These harness defects were corrected explicitly.
- `unic config set` split dots inside quoted instance IDs. It now uses the existing TOML parser.
- A harness expected a failed CLI exit for policy filtering. The existing contract correctly preserves successful local commit and reports a filtered network disposition.
- The shared loopback fixture retained stale-authority sockets. It now compares each session's verified revision with the new state. Comparing against the previous peer snapshot was also wrong: the first authorized snapshot could close an already fresh session. Session-owned authority fixes that race.
- Service readiness preceded the directory adapter's baseline/ready boundary. A deterministic two-second fixture delay reproduced the first-clipboard timeout: transport diagnostics showed successful receive, history contained the remote entry, and Received stayed available. Existing startup semantics intentionally ignore receipts before adapter readiness. The workflow now requires a successful production clipboard read before sending. That response is possible only after baseline/ready and command handling complete. The same delayed scenario then passed without resending content.
- The final matrix exposed an RPC handler scheduling race: an admitted image completed with `permission_denied` because its child part handler ran before its parent published receive state. The proxy now waits on the already admitted grant until the parent publishes its state. It still checks the actual parent ID and receive ID, consumes grants once, and retains cancellation and expiry bounds. The first failed matrix and its secret-free diagnostics remain retained.
- Native failure artifacts preserve the ambiguous external Open button, a null-versus-default enabled assertion, and a harness navigation error. External list buttons now have unique accessible names; native assertions respect omitted-enabled semantics and navigate back to history before device management.
- macOS temporary-directory aliases triggered existing cache confinement. Verification uses `TMPDIR=/private/tmp`.
- The preinstalled lint binary could not read this repository's configuration. Verification uses pinned golangci-lint v2.15.0 with Go 1.27.3.
- The first earlier headless checkpoint detected source mutation while formatting was still running. Its failure manifest was retained; the immutable-source rerun passed.

## Reproduction

```sh
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_ARTIFACTS=/absolute/new/evidence \
  go test ./e2e/cli -run '^TestSyncPluginWorkflow|^TestCLI/sync-local-only$' -count=1 -timeout=5m -v
TMPDIR=/private/tmp UNIC_E2E_ARTIFACTS=/absolute/new/channel-evidence \
  go test -race ./engine -run '^TestExternalChannelInstanceWorkflow$' -count=1 -v
make desktop-e2e CASE=external-sync-workflow
make headless-e2e
TMPDIR=/private/tmp go test -race ./...
```

The real CLI workflow builds and installs one pinned loopback SDK executable through an isolated GitHub fixture. Both services disable Tailcat. The multi-instance case configures two instances per side, uses different local IDs, checks distinct runtime markers and secret redaction, tests code/ticket expiry/cancellation/wrong issuer routing, compares fingerprints, rejects stale approvals, sends exact text/PNG/files both ways, checks deduplication/partial failure/child crash, exercises binding CAS, invalidates changed configuration authorization, revokes/restarts/disables and checks child exit. No content resend loop is used.

The canonical native case seeds only a digest-verified fixture installation, then drives instance creation, enablement/restart, policy, inline pairing approval and results through the production UI. Real installation remains a separately established CLI layer. The loopback fixture is unencrypted, loopback-only and intended only for synthetic acceptance. It is not a production network transport.

## Validation hold

The external-plugin CLI matrix passed twice after the admitted-receive scheduling fix, followed by a separate artifact-producing run. The final canonical native Desktop case, production headless acceptance, full race suite, pinned lint and formatting passed.

One earlier full race run failed `TestSubmitDeliversEverything`: the built-in Tailcat target completed with `unavailable: the transfer failed`. The test did not retain the underlying transport cause. The first log is retained. The exact original base (`894790a3a4dc512ab622b0cf9120895251955173`) passed 100 isolated runs of that test and two full Engine race suites. These comparisons do not establish that the failure is unrelated. Its cause and relevance remain unresolved. Delivery is held under the repository's failure rule, despite the passing final run. Do not report overall acceptance complete or merge this branch until this failure is explained or established as unrelated. On 2026-11-22, the user explicitly authorized pushing this branch to create a draft PR while retaining the merge hold. This authorization does not close the failure or authorize release or deployment.

## Evidence

The 2026-11-22 [readiness-race investigation](tailcat-connected-publication.md)
reproduced the same public failure with a controlled two-Engine transfer. The
underlying response in that reproduction was `not_found`: Engine had accepted
`connected` before the backend registered its session. The affected production
file was identical in the original base and pre-fix PR source. A publication
lock fixes this demonstrated pre-existing race. The original and controlled
cases each passed 20 times after the fix, followed by a full race suite and
production headless acceptance. The original historical log has no underlying
response code, so these results do not uniquely attribute that historical
invocation. The draft and merge hold remain in force.

The final small receipt is `.cache/sync-plugin-workflow/instances/receipt.json`. It records implementation/documentation SHAs, exact per-layer verdicts, command replays, program hashes, canonical Desktop report paths, retained first failures, and cleanup. `SHA256SUMS` binds retained evidence. Only source, reference fixtures and English documentation are committed. No database, ticket, credential, binary, source snapshot or large log is committed. Native screenshots remain in ignored canonical output directories. No push, PR, deployment, VM or personal clipboard is involved.

## Integration with the BoardUI v2 branch

Merged `origin/v2` at `e7f76740499ac6c0dc1be46f3269ebce6a9638a3` into the workflow branch. Conflict resolution retains the native `desktopkit` presentation, runtime instance configuration/status, Tailcat disable control, local pairing selectors, pending-path disclosure and per-backend history results. The external workflow case uses the current device dialog and returns to history after approval closes that dialog.

The merged source passed the real CLI singleton/multiple-instance matrix, canonical native `external-sync-workflow`, `go test -race ./...`, pinned golangci-lint v2.15.0 and formatting. The native report is `.build/desktop-e2e/20261121T132901Z-801cd416/index.html`. Merge evidence is retained under `.cache/merge-v2/`. These passing merge checks do not explain or close the earlier Tailcat failure described under Validation hold. Native OS clipboard/paste and user visual approval remain separate.
