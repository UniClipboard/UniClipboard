package daemonproc

import (
	"unsafe"

	"golang.org/x/sys/unix"
)

const procPidPathInfoMaxSize = 4 * 1024

// processExe uses proc_pidpath via the PROC_PIDPATHINFO syscall.
func processExe(pid uint32) (string, bool) {
	buf := make([]byte, procPidPathInfoMaxSize)
	n, err := procPidPath(int(pid), buf)
	if err != nil || n <= 0 {
		return "", false
	}
	return string(buf[:n]), true
}

func procPidPath(pid int, buf []byte) (int, error) {
	const procInfoCallPidInfo = 2
	const procPidPathInfo = 11
	r, _, errno := unix.Syscall6(unix.SYS_PROC_INFO, procInfoCallPidInfo, uintptr(pid), procPidPathInfo, 0,
		uintptr(unsafe.Pointer(&buf[0])), uintptr(len(buf)))
	if errno != 0 {
		return 0, errno
	}
	return int(r), nil
}
