//go:build e2e && linux

package main

import "os"

// UC_E2E_PAC_BUS_ADDRESS points the PAC-helper supervisor at another bus than the application's own (see pacBusAddress).
func init() {
	if addr := os.Getenv("UC_E2E_PAC_BUS_ADDRESS"); addr != "" {
		pacBusAddress = func() string { return addr }
	}
}
