//go:build unix

package ui

import (
	"os"
	"os/signal"
	"syscall"
)

// raiseInterrupt terminates the process with the default SIGINT disposition.
func raiseInterrupt() {
	signal.Reset(os.Interrupt)
	syscall.Kill(os.Getpid(), syscall.SIGINT)
	select {}
}
