# External sync plugin workflow

## Scope and baseline

Worktree: `7d156c15-73dc-4c64-a305-83836e301bd7`.
Branch: `feat/sync-plugin-workflow`.
Base: `origin/v2` at `894790a3a4dc512ab622b0cf9120895251955173`.
`origin/HEAD` points to `origin/main`; this task does not use that base.

This is an implementation checkpoint, not completion of the full multi-instance workflow.
No push, PR, deployment, VM or personal clipboard is involved.

## Failure inventory specified before implementation

The task-local plan was written before production changes. Its failure inventory covers: unsupported or misclassified plugins; disabled Tailcat fallback; shared instance settings/processes/directories; wrong-instance tickets; new bindings inheriting device trust; expired or changed proofs and approvals; duplicate device rows; policy/global-disable bypass; explicit-send privilege bypass; cross-backend result corruption and duplicate application; uncertain replay after restart; surviving child processes or resources; secret disclosure; and non-P2P backends forced into device pairing.

The first failure is retained. Each identified failure permits at most two diagnostic reruns of ten minutes each. Changed scenarios and new failures receive separate evidence identities. No send is repeated until it happens to arrive.

## Implemented checkpoint

- `builtin.tailcat` can be disabled. The service assembles no implicit replacement. Local capture, history and text Actions remain usable. Enablement changes require restart and the configuration reports that boundary.
- Installed Go backends can receive standard text/image/file/automatic-file-threshold policies through the existing TOML and Engine policy path. Lua list entries still cannot declare a sync policy. The Engine remains the policy authority.
- `unic config set` parses quoted instance IDs with the existing TOML parser. It validates the complete result before saving.
- `GET /v1/sync-backends` projects assembled backend IDs, implementation IDs, transport kinds, external-channel classification and device-pairing support without addresses or tickets. It is an assembly/capability projection, not a promise of current network health.
- `POST /v1/code` and `POST /v1/pairing-ticket` accept optional `backend_id`. Omission retains the default Tailcat behavior. Unknown or unavailable selections do not fall back.
- `unic device backends` prints that projection. `unic device addr --backend ID` exports a ticket for the selected backend.
- Desktop Plugins exposes the same standard policy controls for installed Go sync backends. Tailcat enablement is available on its list and detail. The invitation task selects only device-pairing backends. External channels disclose their plugin-owned configuration path. Object-store device protocols remain unsupported.
- Existing Engine approval, identity and per-backend history results are reused. This checkpoint changes no trust or cryptographic protocol.

## Reproduction and replay

```sh
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_ARTIFACTS=/absolute/new/evidence \
  go test ./e2e/cli -run '^TestSyncPluginWorkflow$' -count=1 -v
TMPDIR=/private/tmp UNIC_E2E=1 \
  go test ./e2e/cli -run '^TestCLI/sync-local-only$' -count=1 -v
make desktop-e2e CASE=data-binding
```

The workflow builds the existing loopback TCP fixture as a real external Go SDK process and installs a pinned release archive through the isolated GitHub installation fixture. Both real services disable Tailcat. Both sides compare the same synthetic fingerprint and explicitly approve their own pending requests. Assertions cover independent history results, bidirectional text/PNG/files with exact bytes, per-instance policy, global disable, revocation, restart and process exit. The test does not replace failed sends with retries.

The shared reference fixture lives in `plugins/examples/loopback`. Existing Engine process acceptance uses the same implementation. It is loopback-only, unencrypted and bounded to small synthetic payloads. It is not a production network protocol or WebDAV/S3 implementation.

## Failures observed during development

1. The first local-only run failed during startup because configuration rejected Tailcat disablement. The repaired run passed capture, Action, restart and unavailable pairing assertions.
2. The first external fixture build used an obsolete SDK entry-point name. The fixture now uses `plugin.Serve`.
3. The first join request used `address` instead of the existing `addr` field. This test-harness error was corrected without changing the service route.
4. The policy step exposed the CLI dotted-key parser defect. The TOML parser now preserves the quoted backend ID.
5. A test expected a nonzero CLI exit for a policy-filtered commit. The existing CLI correctly reports successful local commit and a filtered network disposition. Assertions now inspect the disposition.
6. A file API fixture omitted its required name. The fixture now supplies name and size and verifies received bytes.
7. The first broad race run used the macOS temporary-directory alias. Existing cache-root confinement rejected that alias. A bounded rerun with `TMPDIR=/private/tmp` passed the affected service host tests. Two obsolete rejection assertions were identified; the Tailcat assertion was removed, while the Lua restriction remains enforced.
8. The preinstalled golangci-lint was incompatible with the repository configuration. The pinned repository version (`v2.15.0`) was built with the already installed Go 1.27.3 toolchain. Pinned lint then passed.
9. The first production headless acceptance rejected source mutation while import formatting was still changing. Its failure manifest is preserved. Rerun only after code changes stop; this failure does not establish a product behavior defect.

## Pending contract decision

The device contract section 1 requires implementation identity and instance identity to be distinct. Section 3 requires a locally selected compatible instance; remote and local instance IDs need not match. The sync-backend contract section 1 instead says `backend.bind.backend_id` equals handshake `Spec.ID`. The Engine contract section 5 currently specifies joining on the ticket's exact instance ID. Live code follows that restriction: `engine/plugins.go` keys directories and runtime backends by `Spec.ID`, and `engine/pairing.go:joinVia` looks up `t.Backend` directly.

Proposed resolution: keep handshake `Spec.ID` as the implementation identity; pass the local runtime instance ID in `backend.bind.backend_id`; add an explicit assembly instance ID and local pairing selection; persist settings and private data per instance. Keep RPC framing and cryptographic protocol version 1. No implementation of this conflicting public-contract change is included while approval is pending.

## Remaining acceptance

NOT RUN / not implemented in this checkpoint: two independent instances of one implementation; different sender/receiver instance IDs; binding-level CLI/GUI pause/remove controls and full path projection; full non-Tailcat canonical native Desktop flow; ticket expiry and stale-revision process matrix; dual-backend partial failure and duplicate-application matrix; crash/restart and in-flight revocation barriers; production third-party channel login and object-store protocols. The data-binding Desktop case only verifies the existing production binding and is not evidence for those missing scenarios. Native visual approval remains separate.

## Validation results

Implementation commit: `1c7256c08fa52dae67d91a1069a5b2606c68de36`.

| Layer | Result | Evidence / limit |
|---|---|---|
| Real external CLI/service/RPC workflow | PASS | `.cache/sync-plugin-workflow/external-workflow-final.json`; Tailcat disabled on both sides; exact text/PNG/file checks; unknown backend rejection; policy/global pause; revocation; restart; external disable and zero-backend mode; child exit |
| Zero-backend capture/history/Action/restart | PASS | `TestCLI/sync-local-only` |
| Full race suite | PASS | `TMPDIR=/private/tmp go test -race ./...`; ordinary opt-in E2E entries remain skipped |
| Affected packages after final policy correction | PASS | Host, localconfig, desktopui and Engine race tests |
| Production untagged headless acceptance | PASS | `.cache/sync-plugin-workflow/headless-final.json`; source unchanged; production program hashes; separate migration opt-ins NOT RUN |
| Canonical Desktop data binding | PASS | `.build/desktop-e2e/20261121T101422Z-1d1aca79/index.html`; production process/software-view binding; not external-backend native interaction |
| Pinned lint / formatter | PASS | golangci-lint v2.15.0; `0 issues`; empty formatter diff |
| Production Desktop build | PASS | Local macOS arm64 build; no native interaction claim |
| Full native external-backend Desktop workflow | NOT RUN | Pending remaining implementation |
| Multi-instance and dual-backend matrix | NOT RUN | Pending contract decision and implementation |

The small close receipt is `.cache/sync-plugin-workflow/receipt.json`. `SHA256SUMS` binds retained evidence; `source-sha256.json` contains per-file hashes without source snapshots. The source fixture and replay commands are committed. Test profiles and loopback services are removed. Backend child PIDs observed before shutdown are checked for exit. Headless binary artifacts are removed after preserving their hashes. No DB, ticket, credential, binary, source snapshot or large log is committed.

Full workflow delivery remains incomplete. It requires the explicit contract decision and the remaining implementation/acceptance above.
