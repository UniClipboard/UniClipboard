//go:build !windows

package main

import (
	"fmt"
	"os"
	"os/exec"
	"regexp"
	"runtime"
	"strconv"
	"strings"
)

func machineInfo(root string) (string, uint64, error) {
	var id string
	if runtime.GOOS == "darwin" {
		b, err := exec.Command("ioreg", "-rd1", "-c", "IOPlatformExpertDevice").Output()
		if err != nil {
			return "", 0, err
		}
		m := regexp.MustCompile(`"IOPlatformUUID"\s*=\s*"([^"]+)"`).FindSubmatch(b)
		if len(m) != 2 {
			return "", 0, fmt.Errorf("no machine identity")
		}
		id = string(m[1])
	} else if runtime.GOOS == "linux" {
		b, err := os.ReadFile("/etc/machine-id")
		if err != nil {
			return "", 0, err
		}
		id = strings.TrimSpace(string(b))
	} else {
		return "", 0, fmt.Errorf("unsupported operating system")
	}
	if id == "" {
		return "", 0, fmt.Errorf("empty machine identity")
	}
	b, err := exec.Command("df", "-Pk", root).Output()
	if err != nil {
		return "", 0, err
	}
	lines := strings.Split(strings.TrimSpace(string(b)), "\n")
	fields := strings.Fields(lines[len(lines)-1])
	if len(fields) < 4 {
		return "", 0, fmt.Errorf("disk probe failed")
	}
	n, err := strconv.ParseUint(fields[3], 10, 64)
	return id, n * 1024, err
}
