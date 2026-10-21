//go:build !darwin && !windows

package main

// Linux never invokes the probe; unknown is conservative on other unsupported hosts.
func probeVisualCapabilities() evidenceClass { return evidenceUnknown }
