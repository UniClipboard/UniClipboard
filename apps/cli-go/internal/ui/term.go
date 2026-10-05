package ui

import (
	"os"

	"golang.org/x/term"
)

func terminalWidth() int {
	if w, _, err := term.GetSize(int(os.Stderr.Fd())); err == nil && w > 0 {
		return w
	}
	return 80
}
