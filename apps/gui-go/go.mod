module github.com/UniClipboard/UniClipboard/apps/gui-go

go 1.26.0

toolchain go1.27.1

require (
	github.com/UniClipboard/UniClipboard/packages/desktop-host-go v0.0.0
	github.com/wailsapp/wails/v3 v3.0.0-beta.28
)

require (
	github.com/adrg/xdg v0.5.3 // indirect
	github.com/coder/websocket v1.8.15 // indirect
	github.com/go-ole/go-ole v1.3.0 // indirect
	github.com/godbus/dbus/v5 v5.2.2 // indirect
	github.com/mattn/go-colorable v0.1.14 // indirect
	github.com/mattn/go-isatty v0.0.20 // indirect
	golang.org/x/sys v0.48.0 // indirect
)

replace github.com/UniClipboard/UniClipboard/packages/desktop-host-go => ../../packages/desktop-host-go
