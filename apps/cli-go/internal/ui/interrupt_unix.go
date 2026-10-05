//go:build unix

package ui

import (
	"os"
	"os/signal"
	"syscall"
	"time"
)

// raiseInterrupt sends SIGINT to this process like console's raise(SIGINT).
// With the default disposition the Go runtime terminates the process by the
// signal; when SIGINT is ignored (inherited) or handled by the command, the
// process continues and the caller sees the interrupted read.
func raiseInterrupt() {
	if signal.Ignored(os.Interrupt) {
		return
	}
	syscall.Kill(os.Getpid(), syscall.SIGINT)
	// Termination happens on the runtime's signal thread; give it time
	// before treating the signal as handled.
	time.Sleep(200 * time.Millisecond)
}
