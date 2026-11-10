# 0018 CLI implementation receipt

## Delivery

Rebased the session onto `origin/v2` at `edad4381b8a080d583c7a558c517b2d671c8fdf3`. Preserved the original design and alias changes. The working branch is `cli-release-channels`. The initial implementation commit is `771ebfa399f7551b615c3583efc053e0e1615cf9`. The final process replay used clean commit `84decc42acae489bb4c4a608c71f2346d4a76148`, including the npm daemon command fix and partial-publication acceptance.

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

- CLI process E2E: 25 checks passed using actual Go builds, npm pack/install and a disposable npm registry.
- Native archive smoke: canonical programs and the relative alias reported the exact build identity.
- npm installation: `unic` and `uniclip` forwarded arguments and failing exit codes. The installed `unicd` command executed the exact service build.
- Publication: alpha, direct stable, promotion without uploads, alpha retention, retries after a platform package succeeded and the main upload failed, incomplete target rejection, integrity conflicts and downgrade rejection passed.
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

## V1 alpha channel transition follow-up

The next CLI release is 2.0.0 build 1. The npm main package currently selects 1.6.2-alpha.1 for alpha and 1.6.1 for latest. The five platform package channels were checked separately. The release comparator now accepts known v1 alpha/beta/rc versions only when the candidate product version is strictly higher. Unknown or same-base legacy tags still fail before mutation. This changes registry selection only; it does not add runtime v1 compatibility.

The expanded process E2E passed 44 checks using actual 2.0.0 packages and a local registry. It verified the old-alpha transition, preserved v1 stable, ambiguous-tag rejection and all previous release/migration cases. The [snapshot receipt](baselines/0018-v1-alpha-transition.json) records the exact tested working-source hashes; this local candidate was dirty and is not a publicly releasable candidate. Ruff and actionlint passed.

Public release is still pending: the release branch has not been pushed, remote five-target builds have not run, and the new workflow OIDC permissions are unverified. No package was published or channel changed on npm.
