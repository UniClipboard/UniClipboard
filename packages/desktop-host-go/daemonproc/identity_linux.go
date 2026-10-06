package daemonproc

import (
	"fmt"
	"os"
	"strings"
	"syscall"
)

func processExe(pid uint32) (string, bool) {
	link := fmt.Sprintf("/proc/%d/exe", pid)
	exe, err := os.Readlink(link)
	if err != nil {
		return "", false
	}
	var st syscall.Stat_t
	if syscall.Stat(link, &st) == nil && st.Nlink == 0 {
		exe = strings.TrimSuffix(exe, " (deleted)")
	}
	return exe, true
}
