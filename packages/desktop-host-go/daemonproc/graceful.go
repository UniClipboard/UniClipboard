package daemonproc

import (
	"context"
	"errors"
	"log"
	"time"
)

// GracefulStopWindow is how long a daemon gets to exit after `POST /lifecycle/graceful-stop` before it is
// terminated. It bounds the request and the exit wait together.
const GracefulStopWindow = 3 * time.Second

// stopSteps are the process operations the graceful-then-terminate sequence composes, injected so the ordering
// can be verified without a real process.
type stopSteps struct {
	requestGraceful func(ctx context.Context) error
	waitExit        func(pid uint32, timeout time.Duration) (bool, error)
	terminate       func(pid uint32, timeout time.Duration) error
	logf            func(format string, args ...any)
}

// StopDaemon stops the daemon process. Where the platform has no catchable stop signal (Windows: the only stop is
// TerminateProcess, which kills the process before its shutdown task runs), it first asks the daemon to stop via
// requestGraceful and waits up to GracefulStopWindow for it to exit, terminating only if it did not. Where
// TerminateAndWait already delivers a graceful signal (Unix SIGTERM), that signal is the stop.
func StopDaemon(pid uint32, requestGraceful func(ctx context.Context) error, terminateTimeout time.Duration) error {
	if !gracefulStopFirst {
		return TerminateAndWait(pid, terminateTimeout)
	}
	return stopGracefullyThenTerminate(stopSteps{
		requestGraceful: requestGraceful,
		waitExit:        waitProcessExit,
		terminate:       TerminateAndWait,
		logf:            log.Printf,
	}, pid, terminateTimeout)
}

func stopGracefullyThenTerminate(s stopSteps, pid uint32, terminateTimeout time.Duration) error {
	if pid == 0 {
		return errors.New("refusing to stop pid 0")
	}
	ctx, cancel := context.WithTimeout(context.Background(), GracefulStopWindow)
	defer cancel()
	deadline, _ := ctx.Deadline()

	if err := s.requestGraceful(ctx); err != nil {
		s.logf("graceful stop request to daemon pid %d failed, terminating it: %v", pid, err)
		return s.terminate(pid, terminateTimeout)
	}
	exited, err := s.waitExit(pid, time.Until(deadline))
	if err != nil {
		s.logf("waiting for daemon pid %d to exit after the graceful stop failed, terminating it: %v", pid, err)
		return s.terminate(pid, terminateTimeout)
	}
	if exited {
		return nil
	}
	s.logf("daemon pid %d did not exit within %s of the graceful stop request, terminating it", pid, GracefulStopWindow)
	return s.terminate(pid, terminateTimeout)
}
