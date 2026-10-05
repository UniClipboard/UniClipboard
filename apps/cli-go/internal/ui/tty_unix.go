//go:build unix

package ui

import "os"

func openTTY() (*os.File, error) { return os.OpenFile("/dev/tty", os.O_RDWR, 0) }
