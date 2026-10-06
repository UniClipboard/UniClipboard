//go:build unix

package daemonproc

import "syscall"

// IsPidAlive matches Rust `kill(pid, 0) == 0` (EPERM counts as not alive).
func IsPidAlive(pid uint32) bool { return syscall.Kill(int(pid), 0) == nil }
