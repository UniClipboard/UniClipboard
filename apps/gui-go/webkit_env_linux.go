//go:build linux

package main

import (
	"log"
	"os"
)

// Disable WebKitGTK's DMABUF renderer on Wayland sessions, as the Tauri shell does (crates/uc-tauri/src/run.rs
// `disable_webkit_dmabuf_on_wayland`): it crashes or paints a blank WebView on a number of GPU and compositor
// combinations, most reliably on wlroots compositors (Hyprland, Sway). A clipboard UI gains nothing from the
// GPU-uploaded buffer path. Scope is Wayland only (a non-empty WAYLAND_DISPLAY, since XDG_SESSION_TYPE can say
// "wayland" without a socket) and an explicit user value, including "0", is respected. It runs at package
// initialisation, before Wails creates any WebView, because WebKit reads the variable when the WebView starts.
func init() {
	const name = "WEBKIT_DISABLE_DMABUF_RENDERER"
	if _, set := os.LookupEnv(name); set || os.Getenv("WAYLAND_DISPLAY") == "" {
		return
	}
	if err := os.Setenv(name, "1"); err == nil {
		log.Printf("Wayland session: disabled the WebKit DMABUF renderer (export %s yourself to override)", name)
	}
}
