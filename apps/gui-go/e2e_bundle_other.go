//go:build e2e && !darwin

package main

// There is no application bundle outside macOS; the build-time identity is the reported one.
func bundleIdentifier() string { return bundleID }
