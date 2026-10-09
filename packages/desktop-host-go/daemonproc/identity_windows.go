package daemonproc

import "golang.org/x/sys/windows"

// IsPidAlive matches the Rust SYNCHRONIZE + zero-timeout wait check.
func IsPidAlive(pid uint32) bool {
	h, err := windows.OpenProcess(windows.SYNCHRONIZE, false, pid)
	if err != nil {
		return false
	}
	defer windows.CloseHandle(h)
	event, _ := windows.WaitForSingleObject(h, 0)
	return event == uint32(windows.WAIT_TIMEOUT)
}

// processExe matches Rust, which does not read the executable on Windows.
func processExe(uint32) (string, bool) { return "", false }
