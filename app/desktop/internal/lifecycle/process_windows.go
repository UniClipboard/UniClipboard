package lifecycle

import (
	"os/exec"

	"github.com/uniclipboard/unic/internal/servicelaunch"
)

func configureProcess(cmd *exec.Cmd) { servicelaunch.ConfigureProcess(cmd) }
