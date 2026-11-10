//go:build !windows

package lifecycle

import (
	"os/exec"

	"github.com/uniclipboard/unic/internal/servicelaunch"
)

// configureProcess detaches the service so logging out of the launching terminal does not stop it.
func configureProcess(cmd *exec.Cmd) { servicelaunch.ConfigureProcess(cmd) }
