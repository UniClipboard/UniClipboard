//go:build unic_e2e

package main

import (
	"os"

	"github.com/uniclipboard/unic/internal/clipboardintegration"
	"github.com/uniclipboard/unic/internal/nativeboard"
)

func init() {
	if name := os.Getenv("UNIC_E2E_NAMED_PASTEBOARD"); name != "" {
		openDesktopBoard = func() (clipboardintegration.Board, error) { return nativeboard.NamedForE2E(name) }
	}
}
