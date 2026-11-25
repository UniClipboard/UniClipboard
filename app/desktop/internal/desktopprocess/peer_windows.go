package desktopprocess

import (
	"context"
	"errors"
	"net"
	"os"
	"path/filepath"
	"time"

	"golang.org/x/sys/windows"
)

func peerExecutable(conn net.Conn, server bool) (string, int, error) {
	file, ok := conn.(interface{ Fd() uintptr })
	if !ok {
		return "", 0, errors.New("desktop IPC pipe handle unavailable")
	}
	var pid uint32
	var err error
	if server {
		err = windows.GetNamedPipeClientProcessId(windows.Handle(file.Fd()), &pid)
	} else {
		err = windows.GetNamedPipeServerProcessId(windows.Handle(file.Fd()), &pid)
	}
	if err != nil {
		return "", 0, err
	}
	handle, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return "", 0, err
	}
	defer func() { _ = windows.CloseHandle(handle) }()
	path, err := queryProcessImageName(handle)
	if err != nil {
		return "", 0, err
	}
	return path, int(pid), nil
}

func queryProcessImageName(handle windows.Handle) (string, error) {
	path := make([]uint16, 32768)
	length := uint32(len(path))
	if err := windows.QueryFullProcessImageName(handle, 0, &path[0], &length); err != nil {
		return "", err
	}
	return windows.UTF16ToString(path[:length]), nil
}

// retireStalePeer opens one kernel handle to the named-pipe server PID, verifies
// on that same handle that the process is a replaced/stale generation of
// expected.BinaryPath, and terminates it so the role lock and pipe are released.
func retireStalePeer(ctx context.Context, conn net.Conn, _ string, expected Info) (bool, error) {
	file, ok := conn.(interface{ Fd() uintptr })
	if !ok {
		return false, errors.New("desktop IPC pipe handle unavailable")
	}
	var pid uint32
	if err := windows.GetNamedPipeServerProcessId(windows.Handle(file.Fd()), &pid); err != nil {
		return false, err
	}
	if pid == 0 || int(pid) == os.Getpid() {
		return false, errors.New("cannot retire current process")
	}
	access := uint32(windows.PROCESS_QUERY_LIMITED_INFORMATION | windows.PROCESS_TERMINATE | windows.SYNCHRONIZE)
	handle, err := windows.OpenProcess(access, false, pid)
	if err != nil {
		return false, err
	}
	defer func() { _ = windows.CloseHandle(handle) }()
	rawPath, err := queryProcessImageName(handle)
	if err != nil {
		return false, err
	}
	if resolved, evalErr := filepath.EvalSymlinks(rawPath); evalErr == nil && samePath(resolved, expected.BinaryPath) {
		if hash, herr := fileHash(resolved); herr == nil && hash == expected.BinarySHA256 {
			return false, nil
		}
	} else if !isReplacedExecutable(rawPath, expected.BinaryPath) {
		return false, errors.New("desktop IPC peer executable mismatch")
	}
	if err := windows.TerminateProcess(handle, 0); err != nil {
		if event, werr := windows.WaitForSingleObject(handle, 50); werr != nil || event != windows.WAIT_OBJECT_0 {
			return false, err
		}
		return true, nil
	}
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if err := ctx.Err(); err != nil {
			return false, err
		}
		event, err := windows.WaitForSingleObject(handle, 50)
		if err != nil {
			return false, err
		}
		if event == windows.WAIT_OBJECT_0 {
			return true, nil
		}
	}
	return false, errors.New("timed out waiting for stale desktop role to exit")
}
