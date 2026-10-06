//go:build !darwin

package main

func accessibilityTrusted() bool { return false }
