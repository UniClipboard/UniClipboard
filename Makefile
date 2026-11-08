# Go has no default output directory for `go build`; this puts binaries in dist/.
.PHONY: build test clean
build:
	go build -o dist/ ./app/desktop/cmd/... ./plugins/examples/...
test:
	go test -race ./...
clean:
	rm -rf dist
