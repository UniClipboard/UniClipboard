//go:build e2e && !windows

package quickpanelhelper

import "syscall"

// alive reports whether a process with this pid exists.
func alive(pid int) bool { return syscall.Kill(pid, 0) == nil }
