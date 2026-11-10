# 0018 CLI implementation receipt

## Delivery

Rebased the session onto `origin/v2` at `edad4381b8a080d583c7a558c517b2d671c8fdf3`. Preserved the original design and alias changes. The working branch is `cli-release-channels`. The implementation commit is `771ebfa399f7551b615c3583efc053e0e1615cf9`. The final process replay used this clean committed source.

Implemented the CLI vertical slice:

- Independent product metadata and channel-free version/build identity.
- Early executable identity output for CLI, service and Desktop.
- Versioned CLI archives, relative Unix `uniclip` symlink and Windows command alias.
- Exact npm main/platform package assembly and one launcher for both command names.
- Native build workflow, alpha/direct-stable release and exact-build promotion.
- Full-set integrity preflight, destination read-back and per-package failure receipts.
- Durable GitHub build records with verified source tags, immutable assets and exact-byte recovery before release retries.

The [release architecture](../architecture/releases.md) describes implemented behavior. The [release contract](../contracts/release-contract.md) defines metadata and receipt fields. Decision 0018 stays open for Desktop/mobile implementation.

## Acceptance

The [small check receipt](baselines/0018-cli-local-checks.json) records source hashes and results. The local replay report is `.cache/release-e2e/report.json`; its companion `SHA256SUMS` verifies the report bytes. No binaries or package archives are retained in this delivery.

- CLI process E2E: 22 checks passed using actual Go builds, npm pack/install and a disposable npm registry.
- Native archive smoke: canonical programs and the relative alias reported the exact build identity.
- npm installation: `unic` and `uniclip` forwarded arguments and failing exit codes.
- Publication: alpha, direct stable, promotion without uploads, alpha retention, retries, incomplete target rejection, integrity conflicts and downgrade rejection passed.
- Durable-record command fixture: source-tag creation, immutable staging retries, exact-byte resume and conflict rejection passed. This is not real GitHub API acceptance.
- Production headless E2E: passed in 49.24 seconds.
- Changed Go package lint: passed with the pinned, checksum-verified golangci-lint v2.15.0.
- Changed Go formatting, Python Ruff checks and actionlint: passed.

Full repository checks are not green. `go test -race ./...` failed in 207 tests across Engine, service host and v1 migration with `cannot open default file cache`. An independent archive of the original `edad4381b` source reproduced the same 207 failed test names. Full lint produced 39 issues; original-source lint output was identical. This implementation does not repair those baseline problems.

Replay:

```sh
python3 e2e/release/run.py
make headless-e2e
cd .cache/release-e2e
shasum -a 256 -c SHA256SUMS
```

Use Node 22.23.4 or a supported newer version. Public dist-tag operations use the workflow-pinned npm 11.21.2, not the older npm used by local fixtures.

## Remaining prerequisites

No workflow was dispatched. No production package, GitHub release, R2 object or FlareRelease record was published. Public OIDC/provenance, remote target runners and real GitHub record APIs remain unverified. The initial product metadata is a local candidate seed. Public version allocation, existing npm channel transition and exact workflow authorization still need release preparation.

Desktop immutable CLI consumption, stable CLI gate, signing/installers and R2/FlareRelease publication remain open. Android/iOS native projects and release paths remain open. No GUI, native device, Store or GitCode operations ran.
