//go:build windows

package procmetrics

import (
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

type processMemoryCountersEx struct {
	CB                         uint32
	PageFaultCount             uint32
	PeakWorkingSetSize         uintptr
	WorkingSetSize             uintptr
	QuotaPeakPagedPoolUsage    uintptr
	QuotaPagedPoolUsage        uintptr
	QuotaPeakNonPagedPoolUsage uintptr
	QuotaNonPagedPoolUsage     uintptr
	PagefileUsage              uintptr
	PeakPagefileUsage          uintptr
	PrivateUsage               uintptr
}

var (
	kernel32DLL              = windows.NewLazySystemDLL("kernel32.dll")
	procK32GetProcessMemInfo = kernel32DLL.NewProc("K32GetProcessMemoryInfo")
	procGetTickCount64       = kernel32DLL.NewProc("GetTickCount64")
)

func windowsBootElapsed() time.Duration {
	if err := procGetTickCount64.Find(); err != nil {
		return 0
	}
	r1, _, _ := procGetTickCount64.Call()
	return time.Duration(r1) * time.Millisecond
}

func fileTimeSeconds(ft windows.Filetime) float64 {
	ticks := (uint64(ft.HighDateTime) << 32) | uint64(ft.LowDateTime)
	return float64(ticks) / 1e7
}

func readPlatformSnapshot() Snapshot {
	snap := Snapshot{
		Monotonic:     time.Now(),
		BootElapsed:   windowsBootElapsed(),
		GoHeapBytes:   GoHeapBytes(),
		WakeupsStatus: StatusUnsupported,
	}

	h := windows.CurrentProcess()
	var created, exited, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(h, &created, &exited, &kernel, &user); err != nil {
		snap.CPUStatus = StatusError
	} else {
		snap.CPUTime = fileTimeSeconds(kernel) + fileTimeSeconds(user)
		snap.CPUStatus = StatusOK
	}

	if err := procK32GetProcessMemInfo.Find(); err != nil {
		snap.MemoryStatus = StatusError
		return snap
	}
	var pmc processMemoryCountersEx
	pmc.CB = uint32(unsafe.Sizeof(pmc))
	r1, _, _ := procK32GetProcessMemInfo.Call(uintptr(h), uintptr(unsafe.Pointer(&pmc)), uintptr(pmc.CB))
	if r1 == 0 {
		snap.MemoryStatus = StatusError
		return snap
	}
	bytes := uint64(pmc.PrivateUsage)
	if bytes == 0 {
		bytes = uint64(pmc.WorkingSetSize)
	}
	if bytes == 0 {
		snap.MemoryStatus = StatusError
	} else {
		snap.MemoryFootprintBytes = bytes
		snap.MemoryStatus = StatusOK
	}
	return snap
}
