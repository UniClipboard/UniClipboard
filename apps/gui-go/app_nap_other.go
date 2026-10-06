//go:build !darwin

package main

import "time"

// There is no App Nap outside macOS: Windows and Linux do not suspend the timers of a background process, and
// the one resume event they have (Wails Common.SystemDidWake) is already wired. No scheduler is needed.
func startBackgroundActivity(time.Duration, func()) (stop func()) { return func() {} }
