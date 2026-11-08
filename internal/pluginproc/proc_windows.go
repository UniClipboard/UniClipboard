//go:build windows

package pluginproc

import (
	"os/exec"
	"sync"
	"unsafe"

	"golang.org/x/sys/windows"
)

// Each plugin runs inside its own job object with KILL_ON_JOB_CLOSE, so
// ending the job reclaims the whole process tree, and so does the host dying.
var jobs sync.Map // *exec.Cmd -> windows.Handle

func setProcAttrs(cmd *exec.Cmd) {}

// afterStart puts the started process in a new job. Best effort: without a job
// killTree falls back to ending the process alone.
func afterStart(cmd *exec.Cmd) {
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return
	}
	info := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	info.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation,
		uintptr(unsafe.Pointer(&info)), uint32(unsafe.Sizeof(info))); err != nil {
		_ = windows.CloseHandle(job)
		return
	}
	h, err := windows.OpenProcess(windows.PROCESS_SET_QUOTA|windows.PROCESS_TERMINATE, false, uint32(cmd.Process.Pid))
	if err != nil {
		_ = windows.CloseHandle(job)
		return
	}
	defer windows.CloseHandle(h)
	if err := windows.AssignProcessToJobObject(job, h); err != nil {
		_ = windows.CloseHandle(job)
		return
	}
	jobs.Store(cmd, job)
}

// afterExit releases the job once the process has been reaped.
func afterExit(cmd *exec.Cmd) {
	if v, ok := jobs.LoadAndDelete(cmd); ok {
		_ = windows.CloseHandle(v.(windows.Handle)) // closing kills any survivors
	}
}

// killTree ends the plugin and everything in its job.
func killTree(cmd *exec.Cmd) {
	if v, ok := jobs.Load(cmd); ok {
		_ = windows.TerminateJobObject(v.(windows.Handle), 1)
	}
	if cmd.Process != nil {
		_ = cmd.Process.Kill()
	}
}
