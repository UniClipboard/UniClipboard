//go:build e2e && (!linux || !gtk3)

package main

func windowFrameDetail(_ *HostService, name string) map[string]any {
	return map[string]any{"window": name, "exists": false, "unsupported": true}
}
