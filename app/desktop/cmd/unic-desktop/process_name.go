package main

import "github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"

// roleProcessName returns the short OS process/thread name for a desktop role.
// Names stay within Linux's 15-byte TASK_COMM_LEN-1 limit for PR_SET_NAME.
func roleProcessName(role desktopprocess.Role) string {
	switch role {
	case desktopprocess.Tray:
		return "unic-tray"
	case desktopprocess.GUI:
		return "unic-gui"
	case desktopprocess.QuickPanel:
		return "unic-panel"
	default:
		return ""
	}
}
