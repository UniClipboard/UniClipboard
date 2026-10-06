//go:build !release

package main

const releaseBuild = false

func validateRelease() error { return nil }
