module github.com/UniClipboard/UniClipboard/apps/cli-go

go 1.26.0

toolchain go1.27.1

require (
	github.com/spf13/cobra v1.10.2
	github.com/spf13/pflag v1.0.10
	golang.org/x/sys v0.48.0 // indirect
	golang.org/x/term v0.46.0
)

require (
	github.com/coder/websocket v1.8.15 // indirect
	github.com/inconshreveable/mousetrap v1.1.0 // indirect
)

require github.com/UniClipboard/UniClipboard/packages/desktop-host-go v0.0.0

replace github.com/UniClipboard/UniClipboard/packages/desktop-host-go => ../../packages/desktop-host-go
