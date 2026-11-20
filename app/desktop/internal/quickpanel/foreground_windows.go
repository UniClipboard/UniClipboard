//go:build windows

package quickpanel

// AllowForeground transfers the caller's foreground activation permission to
// its verified panel child before that child restores an external target.
func AllowForeground(pid int) {
	_, _, _ = user32.NewProc("AllowSetForegroundWindow").Call(uintptr(pid))
}
