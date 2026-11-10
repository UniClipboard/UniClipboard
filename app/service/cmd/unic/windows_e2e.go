//go:build unic_e2e

package main

import (
	"os"
	"strconv"
	"time"
)

// Process tests shorten the frozen windows to observe expiry without waiting minutes.
func init() {
	override := func(name string, target *time.Duration) {
		if ms, err := strconv.Atoi(os.Getenv(name)); err == nil && ms > 0 {
			*target = time.Duration(ms) * time.Millisecond
		}
	}
	override("UNIC_E2E_CONNECT_WINDOW_MS", &connectWindow)
	override("UNIC_E2E_SEND_DEADLINE_MS", &sendDeadline)
	override("UNIC_E2E_PAIRING_WINDOW_MS", &pairingWindow)
}
