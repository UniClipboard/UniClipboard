//go:build !linux && !windows

package main

func platformInstallKind() InstallKind { return InstallKindUnknown }
