# Contributing

## Choose the code line

Use `v2` for active development. The retired implementation is preserved on `v1`.
Target v2 pull requests at `v2` until that branch is promoted to `main`.
Read [AGENTS.md](AGENTS.md) and the relevant
[architecture pages](docs/architecture/README.md) before editing.
Application-specific instructions live under `app/<platform>/`.

## Set up

Install the Go version declared in [go.mod](go.mod) and the native prerequisites
listed in the [desktop guide](app/desktop/README.md).

```sh
git clone --branch v2 https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard
go mod download
make build
```

`make build` writes binaries to `dist/`. The CLI and service can also be built
without the graphical host using `make build-headless`.
Rust, Bun and Wails are not prerequisites for this code line.

## Validate changes

Use the existing checks appropriate to the change:

```sh
go test -race ./...
UNIC_E2E=1 go test ./e2e/cli -v
go run ./tools/docscheck
```

Prefer end-to-end acceptance for behavior changes and retain a reproducible
acceptance receipt. The [CLI case catalog](e2e/cli/CASES.md) documents existing
coverage. Native visual acceptance is separate from build and process checks.
Install the version in [.golangci-lint-version](.golangci-lint-version) for linting:

```sh
golangci-lint run ./...
golangci-lint fmt --diff
```

## Submit a change

Keep changes focused. Describe the problem, resulting behavior and checks run.
State checks that were not run. Update architecture pages or contracts when
behavior changes, and follow the verification-marker procedure in AGENTS.md.
Write new documentation in English. Keep secrets and local diagnostics out of
commits. Inspect log archives before attaching them to public issues.
Report vulnerabilities privately using [SECURITY.md](SECURITY.md).

## Branch promotion checklist

When `v2` becomes `main`, update the clone command and branch guidance in this
file. Remove both `target-branch: v2` entries from
[Dependabot configuration](.github/dependabot.yml) so updates follow the default
branch. CodeRabbit already accepts both `v2` and `main` as review targets.
