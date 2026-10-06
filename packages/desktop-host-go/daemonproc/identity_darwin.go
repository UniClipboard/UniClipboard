package daemonproc

import (
	"bytes"

	"golang.org/x/sys/unix"
)

// processExe returns the executable path (or, for processes of other users,
// the kernel process name) without cgo. Rust uses proc_pidpath; only the
// basename prefix matters to IsActiveDaemon, which the 16-byte process name
// preserves for `uniclipd`/`uniclip` and their suffixed sidecar names.
func processExe(pid uint32) (string, bool) {
	if raw, err := unix.SysctlRaw("kern.procargs2", int(pid)); err == nil && len(raw) > 4 {
		path := raw[4:] // skip argc
		if i := bytes.IndexByte(path, 0); i > 0 {
			return string(path[:i]), true
		}
	}
	info, err := unix.SysctlKinfoProc("kern.proc.pid", int(pid))
	if err != nil {
		return "", false
	}
	comm := info.Proc.P_comm[:]
	n := bytes.IndexByte(comm, 0)
	if n < 0 {
		n = len(comm)
	}
	if n == 0 {
		return "", false
	}
	return string(comm[:n]), true
}
