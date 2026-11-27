//go:build !darwin && !windows

package scenarios

import "fmt"

func hideGUIProcess(int) error    { return fmt.Errorf("hideGUIProcess unsupported on this OS") }
func restoreGUIProcess(int) error { return fmt.Errorf("restoreGUIProcess unsupported on this OS") }
