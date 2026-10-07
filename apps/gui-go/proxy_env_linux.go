//go:build linux

package main

import (
	"os"
	"strings"
)

// loopbackProxyBypass is what the Tauri shell has always merged into NO_PROXY (crates/uc-tauri/src/process_environment.rs
// `LOOPBACK_PROXY_BYPASS`). The AppImage bundles libproxy's GIO resolver (17c12), which reads http_proxy/https_proxy/all_proxy
// and, unlike the GNOME resolver's default ignore-hosts, has no built-in loopback exemption: with a proxy variable set the
// WebView's own HTTP/WebSocket connection to the local daemon went to the proxy (observed with a real proxy in the 17c12 stage-2
// run). The daemon inherits the same variables, so the merged list also protects its own loopback requests.
var loopbackProxyBypass = []string{"localhost", "127.0.0.1", "::1"}

// mergeNoProxy returns the union of the given NO_PROXY-style lists (order kept, duplicates dropped) with the loopback hosts
// appended. A wildcard "*" already bypasses everything and is returned alone, as the Tauri shell does.
func mergeNoProxy(values ...string) string {
	var entries []string
	seen := map[string]bool{}
	add := func(entry string) {
		if !seen[entry] {
			seen[entry] = true
			entries = append(entries, entry)
		}
	}
	for _, value := range values {
		for _, entry := range strings.Split(value, ",") {
			entry = strings.TrimSpace(entry)
			if entry == "*" {
				return "*"
			}
			if entry != "" {
				add(entry)
			}
		}
	}
	for _, entry := range loopbackProxyBypass {
		add(entry)
	}
	return strings.Join(entries, ",")
}

// The merge runs at package initialisation, before Wails creates any WebView or this process starts the daemon: both read the
// environment when they start. The user's own NO_PROXY/no_proxy entries are kept.
func init() {
	merged := mergeNoProxy(os.Getenv("NO_PROXY"), os.Getenv("no_proxy"))
	_ = os.Setenv("NO_PROXY", merged)
	_ = os.Setenv("no_proxy", merged)
}
