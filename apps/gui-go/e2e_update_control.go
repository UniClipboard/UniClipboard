//go:build e2e

package main

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
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
//	invoke <label> <command> [<json>]  a host command through Invoke, the path the WebView takes
//	shortcut-press <label> single|leader <a> <b>|second <b>  injected presses (no keyboard event)
//	panel-js <label> <js>   run a script in the quick panel page
//	layer-state <label>     the Layer Shell panel's state as GTK/libgtk-layer-shell report it, and the last placement (Linux)
//	shortcut-state <label>  the registered global shortcuts next to the stored setting and the panel state
//	modifier-script <label> <ms:sel:other ...>  drive the scripted keyboard of the modifier double-tap monitor
//	modifier-state <label>  the monitor's selected modifier, trigger count and panel state
//	autostart-state <label>  the stored auto-start preference next to the OS login item registration (entry path)
//	exit                quit, stopping the daemon (UC_GUI_GO_EXIT_MODE=full) or leaving it
func (s *EvidenceService) watchControlFile(path string) {
	done := 0
	if restartedGUI {
		// A restarted GUI inherits the control file: the commands already run belong to its predecessor.
		if raw, err := os.ReadFile(path); err == nil {
			for _, line := range strings.Split(string(raw), "\n") {
				if strings.TrimSpace(line) != "" {
					done++
				}
			}
		}
	}
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
	case "tray-open-menu":
		// tray-open-menu <label>: SystemTray.OpenMenu, Wails' own path into native NSMenu tracking (a synthesized mouse-down on the status item
		// button; it blocks the main thread inside the tracking loop until the menu is dismissed). No menu callback or snapshot is involved.
		start := time.Now().UnixNano()
		h.tray.tray.OpenMenu()
		_ = s.write(Step{Window: "tray", Step: "tray-open-menu-" + arg, OK: h.tray.menu != nil, Detail: map[string]any{"startNs": start}})
	case "show-main":
		// show-main <label>: Show the main window WITHOUT Focus, for the 17c15 visibility control (the window starts hidden in the e2e build).
		w, ok := h.app.Window.GetByName("main")
		if ok {
			w.Show()
		}
		time.Sleep(500 * time.Millisecond)
		_ = s.write(Step{Window: "main", Step: "show-main-" + arg, OK: ok && w.IsVisible(), Detail: map[string]any{"mainExists": ok, "visible": ok && w.IsVisible(), "focused": ok && w.IsFocused()}})
	case "tray-language-quiet":
		// tray-language-quiet <label> <ms>: wait until no tray language call arrived for ms (the frontend's startup calls come in a burst).
		label, ms, _ := strings.Cut(arg, " ")
		quiet, _ := strconv.Atoi(ms)
		_ = s.write(Step{Window: "tray", Step: "tray-language-quiet-" + label, OK: waitTrayLanguageQuiet(quiet), Detail: map[string]any{"quietMs": quiet}})
	case "tray-language-race":
		// tray-language-race <label> <n>: n concurrent tray language changes released together, alternating zh-CN and en. The
		// menu (root labels and the device submenu title) must end in one language, the one the tray recorded last.
		label, count, _ := strings.Cut(arg, " ")
		n, _ := strconv.Atoi(count)
		if n < 2 {
			n = 2
		}
		start := make(chan struct{})
		var wg sync.WaitGroup
		for i := 0; i < n; i++ {
			language := "zh-CN"
			if i%2 == 1 {
				language = "en"
			}
			wg.Add(1)
			go func() {
				defer wg.Done()
				<-start
				h.tray.setLanguage(language)
			}()
		}
		close(start)
		wg.Wait()
		h.tray.mu.Lock()
		final := h.tray.language
		h.tray.mu.Unlock()
		_ = s.write(Step{Window: "tray", Step: "tray-language-race-" + label, OK: true, Detail: map[string]any{"calls": n, "final": final}})
	case "tray-language-gap":
		// tray-language-gap <label> <ms>: an ARTIFICIAL schedule. Call A (zh-CN) pauses ms between its two steps; call B (en) starts
		// during that pause. Serialized, B waits for A; unserialized, A's second step lands after B's and the menu is mixed.
		label, ms, _ := strings.Cut(arg, " ")
		gap, _ := strconv.Atoi(ms)
		trayLanguageGap.Store(int64(gap))
		var wg sync.WaitGroup
		wg.Add(1)
		go func() { defer wg.Done(); h.tray.setLanguage("zh-CN") }()
		time.Sleep(time.Duration(gap/3) * time.Millisecond)
		h.tray.setLanguage("en")
		wg.Wait()
		h.tray.mu.Lock()
		final := h.tray.language
		h.tray.mu.Unlock()
		// A one-shot that is still armed was not consumed by call A (something else took it, or the call never ran): the overlap did not happen.
		consumed := trayLanguageGap.Load() == 0
		trayLanguageGap.Store(0)
		_ = s.write(Step{Window: "tray", Step: "tray-language-gap-" + label, OK: consumed, Detail: map[string]any{"gapMs": gap, "final": final, "gapConsumed": consumed}})
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
	case "state":
		// A snapshot of the process the launch scenarios compare across second launches.
		_, mainExists := h.app.Window.GetByName("main")
		conn, _ := daemonproc.ReadConnFile()
		detail := map[string]any{"pid": os.Getpid(), "uniqueID": h.singleInstanceID, "mainExists": mainExists, "label": arg}
		if conn != nil {
			detail["daemonPid"] = conn.PID
		}
		if h.helper != nil {
			detail["helperRunning"] = h.helper.Running()
		}
		_ = s.write(Step{Window: "app", Step: "control-state", OK: true, Detail: detail})
	case "layer-state":
		_ = s.write(Step{Window: quickPanelWindowName, Step: "layer-state-" + arg, OK: true, Detail: layerStateDetail(h)})
	case "panel-js":
		// panel-js <label> <javascript>: run a script in the quick panel page (the e2e driver uses it to install key/focus
		// listeners that report to its own loopback listener, to see which layer a key stops at)
		label, script, _ := strings.Cut(arg, " ")
		w, ok := h.app.Window.GetByName(quickPanelWindowName)
		if ok {
			w.ExecJS(script)
		}
		_ = s.write(Step{Window: quickPanelWindowName, Step: "panel-js-" + label, OK: ok})
	case "layer-negative":
		refused, detail := layerNegativeProbe(h)
		_ = s.write(Step{Window: "main", Step: "layer-negative-" + arg, OK: refused, Detail: detail})
	case "restart":
		// The tray's Restart: replace the daemon, then start a new GUI process and exit this one.
		_ = s.write(Step{Window: "app", Step: "control-restart", OK: true, Detail: map[string]any{"pid": os.Getpid()}})
		go h.fullRestart()
	case "invoke":
		// invoke <label> <command> [<json args>]: a host command through the same Invoke the WebView calls.
		label, rest, _ := strings.Cut(arg, " ")
		command, raw, _ := strings.Cut(rest, " ")
		args := map[string]json.RawMessage{}
		if strings.TrimSpace(raw) != "" {
			if err := json.Unmarshal([]byte(raw), &args); err != nil {
				_ = s.write(Step{Window: "app", Step: "invoke-" + label, OK: false, Detail: "bad arguments: " + err.Error()})
				return
			}
		}
		result := h.Invoke(command, args)
		_ = s.write(Step{Window: "app", Step: "invoke-" + label, OK: true, Detail: map[string]any{"command": command, "ok": result.Ok, "error": result.Error, "data": result.Data}})
	case "shortcut-press":
		// shortcut-press <label> single | leader <leader> <second> | second <second>: INJECTED key presses. They call the
		// handlers the OS callback would call (no keyboard event is generated), so they cover the Uni toggle and chord
		// logic but not the OS binding or key delivery.
		label, rest, _ := strings.Cut(arg, " ")
		fields := strings.Fields(rest)
		ok := len(fields) > 0 && h.binder != nil
		if ok {
			switch {
			case fields[0] == "single":
				h.requestPanelToggle()
			case fields[0] == "leader" && len(fields) == 3:
				h.binder.leaderPressed(fields[1], fields[2])
			case fields[0] == "second" && len(fields) == 2:
				h.binder.secondPressed(fields[1])
			default:
				ok = false
			}
		}
		time.Sleep(400 * time.Millisecond) // the panel show/hide completes asynchronously
		_ = s.write(Step{Window: quickPanelWindowName, Step: "shortcut-press-" + label, OK: ok, Detail: line})
	case "shortcut-state":
		// What is bound with the OS (as this host recorded it and as Wails reports it) next to what the daemon stored.
		h.shortcutsMu.Lock()
		recorded := append([]string{}, h.osShortcuts...)
		h.shortcutsMu.Unlock()
		var stored struct {
			KeyboardShortcuts map[string]json.RawMessage `json:"keyboardShortcuts"`
			QuickPanel        quickPanelSettings         `json:"quickPanel"`
		}
		err := h.client.Get(ctx, "/settings", &stored)
		visible := false
		if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
			visible = w.IsVisible()
		}
		_ = s.write(Step{Window: quickPanelWindowName, Step: "shortcut-state-" + arg, OK: err == nil, Detail: map[string]any{
			"recorded": recorded, "wails": h.app.GlobalShortcut.GetAll(), "stored": stored.KeyboardShortcuts[quickPanelShortcutKey],
			"enabled": stored.QuickPanel.Enabled, "panelVisible": visible, "lastShown": h.panel.lastShown.Load(), "panelReady": h.panel.toggle.isReady()}})
	case "modifier-script", "modifier-state":
		s.controlModifier(verb, arg)
	case "autostart-state":
		// The stored preference next to the login item registration (path included), as the settings page would show it.
		_, _ = s.controlQuickPanel("autostart-state:" + arg)
	case "exit":
		_ = s.write(Step{Window: "update", Step: "control-exit", OK: true})
		go func() { time.Sleep(300 * time.Millisecond); h.quit(os.Getenv("UC_GUI_GO_EXIT_MODE") != "full") }()
	default:
		_ = s.write(Step{Window: "update", Step: "control-unknown", OK: false, Detail: line})
	}
}
