// Command mobile-diagnostics verifies local diagnostics through two mobile Engine processes.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/uniclipboard/unic/bindings/mobile"
)

const privateMarker = "private mobile diagnostic fixture never exported"

func must[T any](value T, err error) T {
	check(err)
	return value
}

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func require(ok bool, message string) {
	if !ok {
		panic(message)
	}
}

func configured(state string) bool {
	var value struct {
		Configured bool `json:"configured_enabled"`
	}
	check(json.Unmarshal([]byte(state), &value))
	return value.Configured
}

func process(profile string, writer bool) {
	config := must(json.Marshal(map[string]string{"data_dir": profile, "device_name": privateMarker}))
	m := must(mobile.NewEngine(string(config)))
	defer func() { check(m.Close(nil)) }()
	check(m.Start(nil))
	require(configured(must(m.GetTelemetryState())) == writer, "unexpected telemetry preference after launch")
	if writer {
		require(!configured(must(m.SetTelemetryEnabled(false))), "disabling telemetry did not apply")
	}
	deadline := time.Now().Add(4 * time.Second)
	for {
		logs := m.ExportRecentDiagnosticLogs()
		if strings.Contains(logs, "engine.started") {
			require(!strings.Contains(logs, privateMarker) && !strings.Contains(logs, profile), "private data reached local diagnostic export")
			for _, line := range strings.Split(strings.TrimSpace(logs), "\n") {
				require(json.Valid([]byte(line)), "diagnostics must contain complete JSON lines")
			}
			break
		}
		require(time.Now().Before(deadline), "local log export was unavailable")
		time.Sleep(20 * time.Millisecond)
	}
	if !writer {
		require(configured(must(m.SetTelemetryEnabled(true))), "enabling telemetry did not apply")
	}
}

func main() {
	if len(os.Args) == 3 {
		process(os.Args[2], os.Args[1] == "write")
		return
	}
	dir := must(os.MkdirTemp("", "unic-mobile-diagnostics-"))
	defer os.RemoveAll(dir)
	profile := filepath.Join(must(filepath.EvalSymlinks(dir)), "profile")
	executable := must(os.Executable())
	for _, phase := range []string{"write", "read"} {
		cmd := exec.Command(executable, phase, profile)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		check(cmd.Run())
	}
	fmt.Println(`{"default_enabled":true,"disable_persisted":true,"reenabled":true,"local_log_export":"redacted_jsonl"}`)
}
