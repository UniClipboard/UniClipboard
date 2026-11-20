//go:build !windows

package e2e

import (
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"time"
)

// Observe only. An exited or zombie process cannot retain GUI resources.
func observeProcessExit(pid int) (func(context.Context) error, func(), error) {
	return func(ctx context.Context) error {
		for {
			out, err := exec.CommandContext(ctx, "ps", "-p", strconv.Itoa(pid), "-o", "stat=").Output()
			if err != nil && ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil {
				var exit *exec.ExitError
				if errors.As(err, &exit) && exit.ExitCode() == 1 && strings.TrimSpace(string(out)) == "" {
					return nil
				}
				return fmt.Errorf("observe owned process %d: %w", pid, err)
			}
			if strings.HasPrefix(strings.TrimSpace(string(out)), "Z") {
				return nil
			}
			select {
			case <-ctx.Done():
				return fmt.Errorf("owned process %d did not exit: %w", pid, ctx.Err())
			case <-time.After(80 * time.Millisecond):
			}
		}
	}, func() {}, nil
}
