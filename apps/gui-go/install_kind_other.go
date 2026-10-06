//go:build !linux

package main

func platformInstallKind() string { return "unknown" }
