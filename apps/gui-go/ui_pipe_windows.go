package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
)

const uiPipeArg = "--ui-pipes="

// Windows has no ExtraFiles support. Inherit exactly the two private pipe handles.
func startUICommand(cmd *exec.Cmd, input, output *os.File) error {
	handles := []syscall.Handle{syscall.Handle(input.Fd()), syscall.Handle(output.Fd())}
	for _, handle := range handles {
		if err := syscall.SetHandleInformation(handle, syscall.HANDLE_FLAG_INHERIT, syscall.HANDLE_FLAG_INHERIT); err != nil {
			return err
		}
		defer syscall.SetHandleInformation(handle, syscall.HANDLE_FLAG_INHERIT, 0)
	}
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, AdditionalInheritedHandles: handles}
	cmd.Args = append(cmd.Args, fmt.Sprintf("%s%d,%d", uiPipeArg, input.Fd(), output.Fd()))
	return cmd.Start()
}

func inheritedUIPipes() (*os.File, *os.File, error) {
	for _, arg := range os.Args[1:] {
		if !strings.HasPrefix(arg, uiPipeArg) {
			continue
		}
		values := strings.Split(strings.TrimPrefix(arg, uiPipeArg), ",")
		if len(values) != 2 {
			break
		}
		input, err := strconv.ParseUint(values[0], 10, 64)
		if err != nil || input == 0 {
			break
		}
		output, err := strconv.ParseUint(values[1], 10, 64)
		if err != nil || output == 0 || output == input {
			break
		}
		return openUIPipes(uintptr(input), uintptr(output))
	}
	return nil, nil, errors.New("UI child requires inherited control pipe handles")
}

func prepareUIPipe(fd uintptr) error {
	return syscall.SetHandleInformation(syscall.Handle(fd), syscall.HANDLE_FLAG_INHERIT, 0)
}
