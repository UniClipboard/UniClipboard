//go:build !windows

package main

import "os"

func replaceEffectsFile(from, to string) error { return os.Rename(from, to) }
