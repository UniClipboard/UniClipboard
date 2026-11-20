//go:build !windows

package desktopprocess

import (
	"errors"
	"net"
	"os"
	"strconv"

	"github.com/tailscale/peercred"
)

func peerExecutable(conn net.Conn, _ bool) (string, int, error) {
	cred, err := peercred.Get(conn)
	if err != nil {
		return "", 0, err
	}
	uid, ok := cred.UserID()
	if !ok || uid != strconv.Itoa(os.Geteuid()) {
		return "", 0, errors.New("desktop IPC peer user mismatch")
	}
	pid, ok := cred.PID()
	if !ok {
		return "", 0, errors.New("desktop IPC peer PID unavailable")
	}
	path, err := processExecutable(pid)
	return path, pid, err
}
