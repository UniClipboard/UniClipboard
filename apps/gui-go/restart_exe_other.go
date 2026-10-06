//go:build !linux

package main

import "os"

func restartExecutable() (string, error) { return os.Executable() }
