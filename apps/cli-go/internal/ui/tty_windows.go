package ui

import "os"

func openTTY() (*os.File, error) { return os.OpenFile("CONIN$", os.O_RDWR, 0) }
