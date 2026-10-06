//go:build e2e

package main

import (
	"context"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// watchControlFile is the command channel of the wake/analytics scenarios: the orchestrator appends one command
// per line to UC_GUI_GO_E2E_CONTROL_FILE and each is answered with an evidence step, so it can interleave
// system-wake injections, manual checks and consent changes with its own observations of the feed. Commands:
//
//	wake <n>            post n system wake notifications back to back
//	check               the check_for_update command (a manual check)
//	tray-check          the tray menu's manual check
//	setting <key> on|off  set a general.* flag (usageAnalyticsEnabled, autoCheckUpdate, autoDownloadUpdate)
//	                      through the daemon settings API, the way the settings page does
//	close-updater       close the updater window the scheduler opened (its page makes a check of its own)
//	exit                quit, stopping the daemon (UC_GUI_GO_EXIT_MODE=full) or leaving it
func (s *EvidenceService) watchControlFile(path string) {
	done := 0
	for {
		time.Sleep(100 * time.Millisecond)
		raw, err := os.ReadFile(path)
		if err != nil {
			continue
		}
		var lines []string
		for _, line := range strings.Split(string(raw), "\n") {
			if line = strings.TrimSpace(line); line != "" {
				lines = append(lines, line)
			}
		}
		for ; done < len(lines); done++ {
			s.runControlCommand(lines[done])
		}
	}
}

func (s *EvidenceService) runControlCommand(line string) {
	h := s.host
	verb, arg, _ := strings.Cut(line, " ")
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	switch verb {
	case "wake":
		n, _ := strconv.Atoi(arg)
		var postErr error
		for i := 0; i < n && postErr == nil; i++ {
			postErr = postSystemWake()
		}
		_ = s.write(Step{Window: "update", Step: "control-wake", OK: postErr == nil, Detail: map[string]any{"posted": n}})
	case "check":
		meta, err := h.checkForUpdate(ctx, nil)
		_ = s.write(Step{Window: "update", Step: "control-check", OK: err == nil, Detail: map[string]any{"found": meta != nil}})
	case "tray-check":
		h.checkUpdateFromTray()
		_ = s.write(Step{Window: "update", Step: "control-tray-check", OK: true})
	case "setting":
		key, value, _ := strings.Cut(arg, " ")
		allowed := key == "usageAnalyticsEnabled" || key == "autoCheckUpdate" || key == "autoDownloadUpdate"
		var err error
		if allowed {
			patch := map[string]any{"general": map[string]any{key: value == "on"}}
			err = h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil)
		}
		_ = s.write(Step{Window: "update", Step: "control-setting", OK: allowed && err == nil, Detail: map[string]any{"key": key, "enabled": value == "on"}})
	case "close-updater":
		w, ok := h.app.Window.GetByName(updaterWindowName)
		if ok {
			w.Close()
		}
		_ = s.write(Step{Window: "update", Step: "control-close-updater", OK: ok})
	case "exit":
		_ = s.write(Step{Window: "update", Step: "control-exit", OK: true})
		go func() { time.Sleep(300 * time.Millisecond); h.quit(os.Getenv("UC_GUI_GO_EXIT_MODE") != "full") }()
	default:
		_ = s.write(Step{Window: "update", Step: "control-unknown", OK: false, Detail: line})
	}
}
