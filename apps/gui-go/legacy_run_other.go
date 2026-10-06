//go:build !windows

package main

func (p loginItemPolicy) sweepLegacyRunValue() error { return nil }
