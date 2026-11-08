//go:build windows

package pluginproc

import "os/exec"

func setProcAttrs(cmd *exec.Cmd) {}

// killTree ends the plugin process. Descendants are not tracked on Windows
// yet; a job object is needed for full process-tree reclamation.
func killTree(cmd *exec.Cmd) {
	if cmd.Process != nil {
		_ = cmd.Process.Kill()
	}
}
