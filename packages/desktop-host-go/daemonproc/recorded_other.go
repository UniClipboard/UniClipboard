//go:build !windows

package daemonproc

// recordedProcessIsDaemon is the shared pid-and-binary check where the platform can read the executable.
func recordedProcessIsDaemon(meta PidMetadata) bool { return IsActiveDaemon(meta.PID) }
