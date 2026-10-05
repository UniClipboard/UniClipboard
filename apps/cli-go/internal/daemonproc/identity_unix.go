//go:build unix

package daemonproc

import "syscall"

// isPidAlive matches Rust `kill(pid, 0) == 0` (EPERM counts as not alive).
func isPidAlive(pid uint32) bool { return syscall.Kill(int(pid), 0) == nil }
