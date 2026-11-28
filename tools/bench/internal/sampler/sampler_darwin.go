//go:build darwin

package sampler

import (
	"context"
	"fmt"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

const (
	rusageInfoV4    = 4
	procPidListFDs  = 1
	procPidTaskInfo = 4
)

type machTimebase struct {
	Numer uint32
	Denom uint32
}

// Exact layout of <sys/resource.h> struct rusage_info_v4 (296 bytes).
type rusageInfoV4Struct struct {
	UUID                      [16]byte
	UserTime                  uint64
	SystemTime                uint64
	PkgIdleWkups              uint64
	InterruptWkups            uint64
	Pageins                   uint64
	WiredSize                 uint64
	ResidentSize              uint64
	PhysFootprint             uint64
	ProcStartAbstime          uint64
	ProcExitAbstime           uint64
	ChildUserTime             uint64
	ChildSystemTime           uint64
	ChildPkgIdleWkups         uint64
	ChildInterruptWkups       uint64
	ChildPageins              uint64
	ChildElapsedAbstime       uint64
	DiskioBytesRead           uint64
	DiskioBytesWritten        uint64
	CPUTimeQoSDefault         uint64
	CPUTimeQoSMaintenance     uint64
	CPUTimeQoSBackground      uint64
	CPUTimeQoSUtility         uint64
	CPUTimeQoSLegacy          uint64
	CPUTimeQoSUserInitiated   uint64
	CPUTimeQoSUserInteractive uint64
	BilledSystemTime          uint64
	ServicedSystemTime        uint64
	LogicalWrites             uint64
	LifetimeMaxPhysFootprint  uint64
	Instructions              uint64
	Cycles                    uint64
	BilledEnergy              uint64
	ServicedEnergy            uint64
	IntervalMaxPhysFootprint  uint64
	RunnableTime              uint64
}

// Exact layout of <sys/proc_info.h> struct proc_taskinfo (96 bytes).
type procTaskInfoStruct struct {
	VirtualSize      uint64
	ResidentSize     uint64
	TotalUser        uint64
	TotalSystem      uint64
	ThreadsUser      uint64
	ThreadsSystem    uint64
	Policy           int32
	Faults           int32
	Pageins          int32
	CowFaults        int32
	MessagesSent     int32
	MessagesReceived int32
	SyscallsMach     int32
	SyscallsUnix     int32
	CSW              int32
	ThreadNum        int32
	NumRunning       int32
	Priority         int32
}

type procFDInfoStruct struct {
	FD     int32
	FDType uint32
}

var (
	darwinOnce      sync.Once
	darwinInitErr   error
	darwinTimebase  machTimebase
	procPidRusageFn func(pid int32, flavor int32, buffer unsafe.Pointer) int32
	procPidInfoFn   func(pid int32, flavor int32, arg uint64, buffer unsafe.Pointer, buffersize int32) int32
	machTimebaseFn  func(info unsafe.Pointer) int32
	appKitOnce      sync.Once
	appKitErr       error
)

func ensureDarwinSyms() error {
	darwinOnce.Do(func() {
		lib, err := purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			darwinInitErr = fmt.Errorf("dlopen libSystem.B.dylib: %w", err)
			return
		}
		purego.RegisterLibFunc(&procPidRusageFn, lib, "proc_pid_rusage")
		purego.RegisterLibFunc(&procPidInfoFn, lib, "proc_pidinfo")
		purego.RegisterLibFunc(&machTimebaseFn, lib, "mach_timebase_info")
		if rc := machTimebaseFn(unsafe.Pointer(&darwinTimebase)); rc != 0 || darwinTimebase.Denom == 0 {
			darwinInitErr = fmt.Errorf("mach_timebase_info failed: rc=%d denom=%d", rc, darwinTimebase.Denom)
			return
		}
	})
	return darwinInitErr
}

func machTicksToNS(ticks uint64) uint64 {
	if darwinTimebase.Numer == darwinTimebase.Denom {
		return ticks
	}
	// Avoid uint64 overflow for large tick values by doing 128-bit-safe split if needed.
	q := ticks / uint64(darwinTimebase.Denom)
	r := ticks % uint64(darwinTimebase.Denom)
	return q*uint64(darwinTimebase.Numer) + (r*uint64(darwinTimebase.Numer))/uint64(darwinTimebase.Denom)
}

func sampleProcessNative(pid int) (ProcessSample, error) {
	if pid <= 0 {
		return ProcessSample{}, fmt.Errorf("invalid pid %d", pid)
	}
	if err := ensureDarwinSyms(); err != nil {
		return ProcessSample{}, err
	}

	var ru rusageInfoV4Struct
	if rc := procPidRusageFn(int32(pid), rusageInfoV4, unsafe.Pointer(&ru)); rc != 0 {
		return ProcessSample{}, fmt.Errorf("proc_pid_rusage(%d) failed: rc=%d", pid, rc)
	}

	userNS := machTicksToNS(ru.UserTime)
	sysNS := machTicksToNS(ru.SystemTime)

	ps := ProcessSample{
		PID:                           pid,
		Alive:                         true,
		PrimaryMemoryBytes:            ru.PhysFootprint,
		PhysFootprintBytes:            ru.PhysFootprint,
		LifetimeMaxPhysFootprintBytes: ru.LifetimeMaxPhysFootprint,
		RSSBytes:                      ru.ResidentSize,
		CPUUserNS:                     userNS,
		CPUSystemNS:                   sysNS,
		CPUTotalNS:                    userNS + sysNS,
		IdleWakeups:                   ru.PkgIdleWkups,
		InterruptWakeups:              ru.InterruptWkups,
	}

	var ti procTaskInfoStruct
	if n := procPidInfoFn(int32(pid), procPidTaskInfo, 0, unsafe.Pointer(&ti), int32(unsafe.Sizeof(ti))); n >= int32(unsafe.Sizeof(ti)) {
		ps.Threads = int(ti.ThreadNum)
		if ti.CSW > 0 {
			ps.ContextSwitches = uint64(ti.CSW)
		}
	}

	if fds, err := countProcessFDs(pid); err == nil {
		ps.FDs = fds
	}

	return ps, nil
}

func countProcessFDs(pid int) (int, error) {
	needed := procPidInfoFn(int32(pid), procPidListFDs, 0, nil, 0)
	if needed < 0 {
		return 0, fmt.Errorf("proc_pidinfo(PROC_PIDLISTFDS, %d) probe failed: %d", pid, needed)
	}
	if needed == 0 {
		return 0, nil
	}
	fdSize := int32(unsafe.Sizeof(procFDInfoStruct{}))
	bufSize := needed + 64*fdSize
	buf := make([]byte, bufSize)
	got := procPidInfoFn(int32(pid), procPidListFDs, 0, unsafe.Pointer(&buf[0]), bufSize)
	if got < 0 {
		return 0, fmt.Errorf("proc_pidinfo(PROC_PIDLISTFDS, %d) read failed: %d", pid, got)
	}
	return int(got / fdSize), nil
}

func clipboardSequenceNative() (uint64, error) {
	appKitOnce.Do(func() {
		if _, err := purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
			appKitErr = fmt.Errorf("dlopen Foundation: %w", err)
			return
		}
		if _, err := purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
			appKitErr = fmt.Errorf("dlopen AppKit: %w", err)
			return
		}
	})
	if appKitErr != nil {
		return 0, appKitErr
	}

	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	poolClass := objc.ID(objc.GetClass("NSAutoreleasePool"))
	if poolClass == 0 {
		return 0, fmt.Errorf("NSAutoreleasePool class unavailable")
	}
	pool := objc.Send[objc.ID](objc.Send[objc.ID](poolClass, objc.RegisterName("alloc")), objc.RegisterName("init"))
	defer objc.Send[void](pool, objc.RegisterName("drain"))

	pbClass := objc.ID(objc.GetClass("NSPasteboard"))
	if pbClass == 0 {
		return 0, fmt.Errorf("NSPasteboard class unavailable")
	}
	board := objc.Send[objc.ID](pbClass, objc.RegisterName("generalPasteboard"))
	if board == 0 {
		return 0, fmt.Errorf("NSPasteboard.generalPasteboard unavailable")
	}
	count := objc.Send[int64](board, objc.RegisterName("changeCount"))
	if count < 0 {
		return 0, nil
	}
	return uint64(count), nil
}

type void = struct{}

func hostArchNative() string {
	return runtime.GOARCH
}

func fillHostInfoNative(ctx context.Context, info *HostInfo) {
	if out, err := exec.CommandContext(ctx, "/usr/bin/sw_vers", "-productVersion").Output(); err == nil {
		info.OSVersion = strings.TrimSpace(string(out))
	}
	if out, err := exec.CommandContext(ctx, "/usr/sbin/sysctl", "-n", "machdep.cpu.brand_string").Output(); err == nil {
		info.CPUModel = strings.TrimSpace(string(out))
	}
	if info.CPUModel == "" {
		if out, err := exec.CommandContext(ctx, "/usr/sbin/sysctl", "-n", "hw.model").Output(); err == nil {
			info.CPUModel = strings.TrimSpace(string(out))
		}
	}
	if out, err := exec.CommandContext(ctx, "/usr/sbin/sysctl", "-n", "hw.memsize").Output(); err == nil {
		if mem, perr := strconv.ParseUint(strings.TrimSpace(string(out)), 10, 64); perr == nil {
			info.PhysicalMemoryBytes = mem
		}
	}
	if out, err := exec.CommandContext(ctx, "/usr/sbin/sysctl", "-n", "kern.hv_vmm_present").Output(); err == nil {
		if strings.TrimSpace(string(out)) == "1" {
			info.VirtualMachine = true
		}
	}
}
