# Go has no default output directory for `go build`; this puts binaries in dist/.
.PHONY: build test clean docs-check
build:
	go build -o dist/ ./app/service/cmd/... ./app/desktop/cmd/... ./plugins/examples/...
test:
	go test -race ./...
clean:
	rm -rf dist

# Documentation lifecycle checks (links, archive references, architecture front matter, staleness).
docs-check:
	go run ./tools/docscheck

.PHONY: build-headless package-headless package-desktop headless-e2e
build-headless:
	go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd
package-headless:
	go run ./tools/package
package-desktop:
	go run ./tools/package -desktop
headless-e2e:
	UNIC_HEADLESS_E2E=1 UNIC_HEADLESS_ARTIFACTS=$(CURDIR)/.cache/0015-acceptance go test -race ./e2e/headless -v -count=1

.PHONY: desktop-startup-e2e
desktop-startup-e2e:
	UNIC_DESKTOP_STARTUP_E2E=1 UNIC_DESKTOP_STARTUP_ARTIFACTS=$(CURDIR)/.cache/desktop-startup-e2e go test -race ./app/desktop/e2e -run '^TestStartupProcesses$$' -v -count=1

.PHONY: real-e2e real-e2e-doctor
real-e2e:
	go run ./e2e/real run --lab "$(LAB)" --suite sync
real-e2e-doctor:
	go run ./e2e/real doctor --lab "$(LAB)"

# Nonvisual local TLS acceptance, including production release assembly.
.PHONY: posthog-e2e
posthog-e2e:
	mkdir -p .cache/0019/bin
	go build -race -tags unic_e2e -o .cache/0019/bin/unicd ./app/service/cmd/unicd
	go build -race -o .cache/0019/bin/unic ./app/service/cmd/unic
	TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=$(CURDIR)/.cache/0019/bin UNIC_E2E_ARTIFACTS=$(CURDIR)/.cache/0019/acceptance go test -race ./e2e/cli -run '^TestPostHog' -count=1 -v

.PHONY: sync-observability-e2e
sync-observability-e2e:
	mkdir -p .cache/0020/bin
	go build -race -tags unic_e2e -o .cache/0020/bin/unicd ./app/service/cmd/unicd
	go build -race -o .cache/0020/bin/unic ./app/service/cmd/unic
	TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=$(CURDIR)/.cache/0020/bin UNIC_E2E_ARTIFACTS=$(CURDIR)/.cache/0020/acceptance go test -race ./e2e/cli -run '^TestSyncObservability($$|Faults$$)' -count=1 -v | tee .cache/0020/acceptance.run.log
	rg -q '^--- PASS: TestSyncObservability ' .cache/0020/acceptance.run.log
	rg -q '^--- PASS: TestSyncObservabilityFaults ' .cache/0020/acceptance.run.log

.PHONY: sync-path-e2e
sync-path-e2e:
	mkdir -p .cache/0020/real-bin
	go build -race -tags ts_omit_captiveportal,ts_omit_portmapper -o .cache/0020/real-bin/unicd ./app/service/cmd/unicd
	UNIC_SYNC_PATH_E2E=1 UNIC_SYNC_PATH_BIN=$(CURDIR)/.cache/0020/real-bin UNIC_SYNC_PATH_ARTIFACTS=$(CURDIR)/.cache/0020/real-path go test -race ./e2e/syncpath -run '^TestPassivePaths$$' -count=1 -v | tee .cache/0020/real-path.run.log
	rg -q '^--- PASS: TestPassivePaths ' .cache/0020/real-path.run.log

.PHONY: logs-export-e2e
logs-export-e2e:
	mkdir -p .cache/logs-export/bin
	go build -race -o .cache/logs-export/bin/unic ./app/service/cmd/unic
	TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=$(CURDIR)/.cache/logs-export/bin UNIC_E2E_ARTIFACTS=$(CURDIR)/.cache/logs-export/acceptance go test -race ./e2e/cli -run '^TestLogsExport$$' -count=1 -v | tee .cache/logs-export/run.log
	rg -q '^--- PASS: TestLogsExport ' .cache/logs-export/run.log
