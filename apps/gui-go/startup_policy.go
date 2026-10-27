package main

// shouldOpenAtLaunch is independent of daemon origin and runtime residency.
// A shell handoff preserves an absent window; every explicit user launch opens.
func shouldOpenAtLaunch(backgroundStartup, automatic, backgroundRestart, force bool) bool {
	return force || (!backgroundRestart && !(automatic && backgroundStartup))
}

const quickPanelLaunchArg = "--quick-panel"

// autostartLaunchArg identifies automatic login launches on Windows/Linux and
// LaunchAgent. The macOS SMAppService path instead uses AppKit launch-origin detection.
const autostartLaunchArg = "--autostart"

// secondLaunchAction is what the running GUI does when another launch reaches it.
type secondLaunchAction string

const (
	actionShowMainWindow   secondLaunchAction = "show-main-window"
	actionIgnoreAutostart  secondLaunchAction = "ignore-autostart"
	actionToggleQuickPanel secondLaunchAction = "toggle-quick-panel"
)

func hasArg(args []string, want string) bool {
	for _, a := range args {
		if a == want {
			return true
		}
	}
	return false
}

// classifySecondLaunch applies the Tauri single-instance rule (a second launch surfaces the main window) except for
// the launches that must not: the login item's own launch (a login autostart never pops the window) and the quick
// panel request (it toggles the panel, not the main window).
func classifySecondLaunch(args []string) secondLaunchAction {
	switch {
	case hasArg(args, quickPanelLaunchArg):
		return actionToggleQuickPanel
	case hasArg(args, autostartLaunchArg):
		return actionIgnoreAutostart
	}
	return actionShowMainWindow
}
