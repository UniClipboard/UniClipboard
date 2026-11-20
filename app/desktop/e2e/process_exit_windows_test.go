package e2e

import (
	"context"
	"errors"
	"fmt"
	"time"

	"golang.org/x/sys/windows"
)

// Bind an OS process handle before Quit. PID reuse cannot change that handle.
// This observer never terminates a process.
func observeProcessExit(pid int) (func(context.Context) error, func(), error) {
	handle, err := windows.OpenProcess(windows.SYNCHRONIZE, false, uint32(pid))
	if errors.Is(err, windows.ERROR_INVALID_PARAMETER) {
		return func(context.Context) error { return nil }, func() {}, nil
	}
	if err != nil {
		return nil, nil, err
	}
	closeHandle := func() { _ = windows.CloseHandle(handle) }
	return func(ctx context.Context) error {
		for {
			result, err := windows.WaitForSingleObject(handle, 0)
			if err != nil {
				return err
			}
			if result == windows.WAIT_OBJECT_0 {
				return nil
			}
			select {
			case <-ctx.Done():
				return fmt.Errorf("owned process %d did not exit: %w", pid, ctx.Err())
			case <-time.After(80 * time.Millisecond):
			}
		}
	}, closeHandle, nil
}
