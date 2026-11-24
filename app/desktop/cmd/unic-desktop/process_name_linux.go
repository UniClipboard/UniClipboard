//go:build linux

package main

import (
	"runtime"
	"unsafe"

	"golang.org/x/sys/unix"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
)

// setRoleProcessName labels the Linux main thread/process (/proc/<pid>/comm)
// without altering /proc/<pid>/exe, preserving desktopprocess peer verification.
func setRoleProcessName(role desktopprocess.Role) {
	if name := roleProcessName(role); name != "" {
		setProcessName(name)
	}
}

func setProcessName(name string) {
	raw, err := unix.ByteSliceFromString(name)
	if err != nil {
		return
	}
	_ = unix.Prctl(unix.PR_SET_NAME, uintptr(unsafe.Pointer(&raw[0])), 0, 0, 0)
	runtime.KeepAlive(raw)
}
