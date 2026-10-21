// Command standin is a test double of the native quick panel helper for the protocol E2E. It speaks the helper's
// stdin/stdout protocol (ADR-013 section 10) and holds a set of bindings in memory; it registers nothing with the
// OS. Its behaviour for each command is read from the JSON file named by STANDIN_CONTROL, so the test can change
// it while the process runs. Every received line and every answer is appended to STANDIN_LOG.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"slices"
	"time"
)

type control struct {
	Refuse  []string `json:"refuse"`  // bindings it refuses (it then keeps what it held)
	DelayMs int      `json:"delayMs"` // answer this late (it applies first)
	Silent  bool     `json:"silent"`  // apply, but never answer
	CrashOn string   `json:"crashOn"` // exit with code 3 the first time it is asked for this binding
}

func main() {
	logf, _ := os.OpenFile(os.Getenv("STANDIN_LOG"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	logLine := func(format string, args ...any) { fmt.Fprintf(logf, format+"\n", args...) }
	logLine(`{"started":%d}`, os.Getpid())
	var held []string
	scan := bufio.NewScanner(os.Stdin)
	for scan.Scan() {
		line := scan.Text()
		logLine(`{"pid":%d,"received":%s}`, os.Getpid(), line)
		var cmd struct {
			Command   string   `json:"command"`
			Seq       uint64   `json:"seq"`
			Shortcuts []string `json:"shortcuts"`
		}
		if json.Unmarshal([]byte(line), &cmd) != nil || cmd.Command != "set_shortcuts" {
			continue
		}
		var c control
		if data, err := os.ReadFile(os.Getenv("STANDIN_CONTROL")); err == nil {
			_ = json.Unmarshal(data, &c)
		}
		// Crash once: the marker file keeps the relaunched process from crashing again.
		marker := os.Getenv("STANDIN_CONTROL") + ".crashed"
		if _, err := os.Stat(marker); c.CrashOn != "" && slices.Contains(cmd.Shortcuts, c.CrashOn) && err != nil {
			_ = os.WriteFile(marker, nil, 0o600)
			logLine(`{"pid":%d,"crash":true}`, os.Getpid())
			os.Exit(3)
		}
		reply := map[string]any{"seq": cmd.Seq}
		refused := ""
		for _, b := range cmd.Shortcuts {
			if slices.Contains(c.Refuse, b) {
				refused = b
			}
		}
		if refused != "" {
			reply["error"] = "the shortcut " + refused + " is already registered (stand-in)"
		} else {
			held = slices.Clone(cmd.Shortcuts)
		}
		reply["active"] = held
		out, _ := json.Marshal(map[string]any{"shortcuts": reply})
		if c.Silent {
			logLine(`{"pid":%d,"silent":true,"wouldAnswer":%s}`, os.Getpid(), out)
			continue
		}
		go func(out []byte, delay int) {
			time.Sleep(time.Duration(delay) * time.Millisecond)
			fmt.Println(string(out))
			logLine(`{"pid":%d,"answered":%s,"delayMs":%d}`, os.Getpid(), out, delay)
		}(out, c.DelayMs)
	}
	logLine(`{"pid":%d,"stdinClosed":true}`, os.Getpid())
}
