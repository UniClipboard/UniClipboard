//go:build !windows

package main

import "encoding/json"

// probeHelperShortcuts: only Windows needs the probe; see shortcut_probe_windows.go.
func (h *HostService) probeHelperShortcuts(_, _ map[string]json.RawMessage, _ bool) error { return nil }
