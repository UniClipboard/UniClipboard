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
