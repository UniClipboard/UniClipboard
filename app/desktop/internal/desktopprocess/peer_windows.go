package desktopprocess

import (
	"errors"
	"net"

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
	path := make([]uint16, 32768)
	length := uint32(len(path))
	if err := windows.QueryFullProcessImageName(handle, 0, &path[0], &length); err != nil {
		return "", 0, err
	}
	return windows.UTF16ToString(path[:length]), int(pid), nil
}
