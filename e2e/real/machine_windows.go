package main

import (
	"fmt"
	"os/exec"
	"strconv"
	"strings"
)

func machineInfo(root string) (string, uint64, error) {
	b, err := exec.Command("reg.exe", "query", `HKLM\SOFTWARE\Microsoft\Cryptography`, "/v", "MachineGuid").Output()
	if err != nil {
		return "", 0, err
	}
	var id string
	for _, line := range strings.Split(string(b), "\n") {
		fields := strings.Fields(line)
		if len(fields) >= 3 && fields[0] == "MachineGuid" {
			id = fields[len(fields)-1]
		}
	}
	if id == "" {
		return "", 0, fmt.Errorf("no machine identity")
	}
	script := "[Console]::WriteLine((Get-Item -LiteralPath '" + strings.ReplaceAll(root, "'", "''") + "').PSDrive.Free)"
	b, err = exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", script).Output()
	if err != nil {
		return "", 0, err
	}
	n, err := strconv.ParseUint(strings.TrimSpace(string(b)), 10, 64)
	return id, n, err
}
