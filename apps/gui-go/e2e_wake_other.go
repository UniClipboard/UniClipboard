//go:build e2e && !darwin

package main

import "errors"

func postSystemWake() error { return errors.New("no system wake injection on this platform") }
