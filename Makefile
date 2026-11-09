# Go has no default output directory for `go build`; this puts binaries in dist/.
.PHONY: build test clean docs-check
build:
	go build -o dist/ ./app/desktop/cmd/... ./plugins/examples/...
test:
	go test -race ./...
clean:
	rm -rf dist

# Documentation lifecycle checks (links, archive references, architecture front matter, staleness).
docs-check:
	go run ./tools/docscheck
