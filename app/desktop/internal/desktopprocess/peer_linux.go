package desktopprocess

import (
	"fmt"
	"os"
)

func processExecutable(pid int) (string, error) { return os.Readlink(fmt.Sprintf("/proc/%d/exe", pid)) }
