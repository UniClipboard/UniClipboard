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

# Real-process acceptance for on-demand CLI service lifecycle (0021). Fixture controls are build-tagged.
.PHONY: service-lifecycle-e2e
service-lifecycle-e2e:
	mkdir -p .cache/0021/bin
	go build -race -tags unic_e2e -o .cache/0021/bin/unicd ./app/service/cmd/unicd
	go build -race -tags unic_e2e -o .cache/0021/bin/unic ./app/service/cmd/unic
	TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=$(CURDIR)/.cache/0021/bin UNIC_E2E_ARTIFACTS=$(CURDIR)/.cache/0021/acceptance go test -race ./e2e/cli -run '^TestServiceLifecycle$$' -count=1 -v | tee .cache/0021/run.log
	rg -q '^--- PASS: TestServiceLifecycle ' .cache/0021/run.log

# Cost and residue comparison. BASELINE_SRC is a clean checkout of the baseline commit (git worktree add --detach DIR COMMIT).
.PHONY: service-lifecycle-bench
service-lifecycle-bench:
	test -n "$(BASELINE_SRC)"
	mkdir -p .cache/0021/bench/base .cache/0021/bench/cand
	cd $(BASELINE_SRC) && go build -o $(CURDIR)/.cache/0021/bench/base/ ./app/service/cmd/unic ./app/service/cmd/unicd
	go build -o .cache/0021/bench/cand/ ./app/service/cmd/unic ./app/service/cmd/unicd
	TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=$(CURDIR)/.cache/0021/bin UNIC_BENCH_BASE=$(CURDIR)/.cache/0021/bench/base UNIC_BENCH_CAND=$(CURDIR)/.cache/0021/bench/cand UNIC_E2E_ARTIFACTS=$(CURDIR)/.cache/0021/acceptance go test ./e2e/cli -run '^TestServiceLifecycleResources$$' -count=1 -v | tee .cache/0021/bench.log
	rg -q '^--- PASS: TestServiceLifecycleResources ' .cache/0021/bench.log

# Temporary transfer (0022) against the real browser transfer page code. Needs bun and the
# uc-website checkout (UC_WEBSITE). The page runs the pinned tailcat wasm build under bun.
.PHONY: web-interop-e2e
web-interop-e2e:
	test -n "$(UC_WEBSITE)"
	TMPDIR=/private/tmp UNIC_WEB_INTEROP=1 UC_WEBSITE=$(UC_WEBSITE) UNIC_E2E_ARTIFACTS=$(CURDIR)/.cache/0022/acceptance go test ./e2e/webinterop -run '^TestBrowserInterop$$' -count=1 -v | tee .cache/0022/web-interop.run.log
	rg -q '^--- PASS: TestBrowserInterop ' .cache/0022/web-interop.run.log

# Process-level acceptance of the user-intent CLI and temporary transfer (0022), unic against unic.
.PHONY: transfer-e2e
transfer-e2e:
	mkdir -p .cache/0022
	TMPDIR=/private/tmp UNIC_E2E=1 go test ./e2e/cli -run '^TestCLI$$/^(temporary-transfer|transfer-code|send-to-device)$$' -count=1 -v | tee .cache/0022/transfer.run.log
	rg -q '^--- PASS: TestCLI ' .cache/0022/transfer.run.log
