package daemonproc

import (
	"strings"
	"time"

	"golang.org/x/sys/windows"
)

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

// recordedStartTolerance is how far the process creation time may differ from the `startedAtMs` the daemon wrote
// into `.daemon-pid` (the daemon records it a moment after the process starts).
const recordedStartTolerance = 15 * time.Second

// recordedProcessIsDaemon reports whether the pid in `.daemon-pid` is still the process that wrote it: alive, an
// executable named like the daemon, and created when the record says. A reused pid fails the time check, an unrelated
// program the name check, and a record without a start time (legacy form) cannot be proven, so it is not trusted.
func recordedProcessIsDaemon(meta PidMetadata) bool {
	if meta.StartedAtMs == 0 {
		return false
	}
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.SYNCHRONIZE, false, meta.PID)
	if err != nil {
		return false
	}
	defer windows.CloseHandle(h)
	if event, _ := windows.WaitForSingleObject(h, 0); event != uint32(windows.WAIT_TIMEOUT) {
		return false
	}
	var creation, exit, kernel, user windows.Filetime
	if windows.GetProcessTimes(h, &creation, &exit, &kernel, &user) != nil {
		return false
	}
	created := time.Unix(0, creation.Nanoseconds())
	recorded := time.UnixMilli(int64(meta.StartedAtMs))
	if d := created.Sub(recorded); d > recordedStartTolerance || d < -recordedStartTolerance {
		return false
	}
	buf := make([]uint16, windows.MAX_PATH)
	n := uint32(len(buf))
	if windows.QueryFullProcessImageName(h, 0, &buf[0], &n) != nil {
		return false
	}
	path := windows.UTF16ToString(buf[:n])
	name := path
	if i := strings.LastIndexAny(path, `/\`); i >= 0 {
		name = path[i+1:]
	}
	return isDaemonBinaryName(name)
}
