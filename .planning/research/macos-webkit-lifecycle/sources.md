# Sources: macOS WebKit process lifetime

Inspected on 2026-10-22. Upstream main is reference evidence, not a pinned description of the
installed macOS WebKit binary.

## Existing product source

- apps/gui-go/main.go: main-window close hook, bootstrap and process-level owners.
- apps/gui-go/dock_darwin.go: activation policy and LaunchServices reopen hook.
- apps/gui-go/quick_panel_native.go: default native helper and pre-created WebView fallback.
- apps/gui-go/windows.go: updater window and WebView Quick Panel creation.
- packages/desktop-host-go/quickpanelhelper/quickpanelhelper.go: existing child supervision seam.
- docs/adr/adr-013-desktop-product-settings.md: daemon-owned saved preferences.

## Pinned Wails

Version: github.com/wailsapp/wails/v3 v3.0.0-beta.28.

- pkg/application/webview_window.go: Close, WindowClosing default listener and NativeWindow.
- pkg/application/webview_window_darwin.go: windowNew, default WKWebViewConfiguration and native close.
- pkg/application/webview_window_darwin.m: native window close/dealloc and delegate ownership.
- https://v3.wails.io/reference/window/: public window lifecycle API.

## Apple SDK and primary documentation

The selected SDK was obtained with xcrun --show-sdk-path. Its public WebKit framework headers
contain no all-process termination operation. WKWebsiteDataStore.removeDataStoreForIdentifier
explicitly deletes persistent data. WKWebViewConfiguration.processPool is deprecated because
creating and using multiple instances no longer has any effect.

- https://developer.apple.com/documentation/webkit/wkwebview
- https://developer.apple.com/documentation/webkit/wkwebviewconfiguration/processpool
- https://webkit.org/blog/17333/webkit-features-in-safari-26-0/

## WebKit primary source

- https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/WebsiteData/WebsiteDataStore.cpp
  protectedGlobalDefaultDataStore retains a default store; defaultDataStore initializes it;
  networkProcess retains the network-process proxy. deleteDefaultDataStoreForTesting is test-only.
- https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKWebsiteDataStore.mm
  _terminateNetworkProcess is a private selector.
- https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKProcessPool.h
  Public WKProcessPool has no termination operation and carries the deprecation notice.
- https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/Network/NetworkProcessProxy.cpp
  Network-process lifetime and session removal are independent of native NSWindow close.

## Cross-platform extension (2026-10-22)

- The user requested the same process architecture on macOS, Windows and Linux.
- Go `os/exec.Cmd.ExtraFiles` does not support Windows:
  https://pkg.go.dev/os/exec#Cmd . The installed Go 1.27.1 Windows `syscall/exec_windows.go`
  supports `AdditionalInheritedHandles` and an explicit handle whitelist. Handles must be
  inheritable before StartProcess. The child clears inheritance before starting other programs.
- Pinned Wails `application_options.go` provides Windows/Linux
  `DisableQuitOnLastWindowClosed`. `linux_cgo_gtk3.go` holds the GtkApplication without windows.
- Pinned Wails `application_linux_appid.go` derives the GTK application ID from Options.Name
  unless Linux.ApplicationID is set. The resident and UI need distinct profile-scoped IDs.
  Its single-instance D-Bus name appends `.SingleInstance`, so it does not collide with the GTK ID.
- Windows/Linux reuse the same executable with an internal UI role. Their existing portable
  resolver therefore sees the same executable directory. Linux AppImage UI starts the executable
  inside the existing mount; the resident keeps that mount alive. Only a whole-app restart uses
  the outer AppImage path.
