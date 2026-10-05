//go:build unix && !darwin && !linux

package daemonproc

func processExe(uint32) (string, bool) { return "", false }
