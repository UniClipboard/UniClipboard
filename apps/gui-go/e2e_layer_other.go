//go:build e2e && (!linux || !gtk3)

package main

func layerStateDetail(*HostService) map[string]any { return map[string]any{"panel": false} }

func layerNegativeProbe(*HostService) (bool, map[string]any) { return false, nil }
