//go:build windows

package sampler

import (
	"context"
	"fmt"
	"os"
	"runtime"
	"sync"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

const (
	grGDIObjects  = 0
	grUSERObjects = 1

	memCommit  = 0x1000
	memReserve = 0x2000
	memPrivate = 0x20000
	memMapped  = 0x40000
	memImage   = 0x1000000

	wsSharedBit = 0x100
)

var (
	modKernel32 = windows.NewLazySystemDLL("kernel32.dll")
	modPsapi    = windows.NewLazySystemDLL("psapi.dll")
	modUser32   = windows.NewLazySystemDLL("user32.dll")

	procGetProcessMemoryInfo     = modPsapi.NewProc("GetProcessMemoryInfo")
	procQueryWorkingSet          = modPsapi.NewProc("QueryWorkingSet")
	procGetProcessHandleCount    = modKernel32.NewProc("GetProcessHandleCount")
	procVirtualQueryEx           = modKernel32.NewProc("VirtualQueryEx")
	procIsWow64Process2          = modKernel32.NewProc("IsWow64Process2")
	procGetGuiResources          = modUser32.NewProc("GetGuiResources")
	procEnumWindows              = modUser32.NewProc("EnumWindows")
	procGetWindowThreadProcessId = modUser32.NewProc("GetWindowThreadProcessId")
	procIsWindowVisible          = modUser32.NewProc("IsWindowVisible")
	procGetForegroundWindow      = modUser32.NewProc("GetForegroundWindow")
	procGetClipboardSequenceNum  = modUser32.NewProc("GetClipboardSequenceNumber")
	procGlobalMemoryStatusEx     = modKernel32.NewProc("GlobalMemoryStatusEx")
)

type processMemoryCountersEx struct {
	cb                         uint32
	pageFaultCount             uint32
	peakWorkingSetSize         uintptr
	workingSetSize             uintptr
	quotaPeakPagedPoolUsage    uintptr
	quotaPagedPoolUsage        uintptr
	quotaPeakNonPagedPoolUsage uintptr
	quotaNonPagedPoolUsage     uintptr
	pagefileUsage              uintptr
	peakPagefileUsage          uintptr
	privateUsage               uintptr
}

type processMemoryCountersEx2 struct {
	cb                         uint32
	pageFaultCount             uint32
	peakWorkingSetSize         uintptr
	workingSetSize             uintptr
	quotaPeakPagedPoolUsage    uintptr
	quotaPagedPoolUsage        uintptr
	quotaPeakNonPagedPoolUsage uintptr
	quotaNonPagedPoolUsage     uintptr
	pagefileUsage              uintptr
	peakPagefileUsage          uintptr
	privateUsage               uintptr
	privateWorkingSetSize      uintptr
	sharedCommitUsage          uint64
}

type memoryBasicInformation struct {
	BaseAddress       uintptr
	AllocationBase    uintptr
	AllocationProtect uint32
	PartitionID       uint16
	_                 uint16
	RegionSize        uintptr
	State             uint32
	Protect           uint32
	Type              uint32
}

type memoryStatusEx struct {
	dwLength                uint32
	dwMemoryLoad            uint32
	ullTotalPhys            uint64
	ullAvailPhys            uint64
	ullTotalPageFile        uint64
	ullAvailPageFile        uint64
	ullTotalVirtual         uint64
	ullAvailVirtual         uint64
	ullAvailExtendedVirtual uint64
}

var (
	enumMu              sync.Mutex
	enumOnce            sync.Once
	enumCallbackPtr     uintptr
	enumTargetPID       uint32
	enumForegroundHWND  uintptr
	enumVisibleCount    int
	enumForegroundCount int
)

func getEnumWindowsCallback() uintptr {
	enumOnce.Do(func() {
		enumCallbackPtr = windows.NewCallback(func(hwnd uintptr, _ uintptr) uintptr {
			var ownerPID uint32
			_, _, _ = procGetWindowThreadProcessId.Call(hwnd, uintptr(unsafe.Pointer(&ownerPID)))
			if ownerPID == enumTargetPID {
				vis, _, _ := procIsWindowVisible.Call(hwnd)
				if vis != 0 {
					enumVisibleCount++
				}
				if hwnd == enumForegroundHWND && enumForegroundHWND != 0 {
					enumForegroundCount++
				}
			}
			return 1 // continue enumeration
		})
	})
	return enumCallbackPtr
}

func sampleProcessNative(pid int) (ProcessSample, error) {
	if pid <= 0 {
		return ProcessSample{}, fmt.Errorf("invalid pid %d", pid)
	}
	access := uint32(windows.PROCESS_QUERY_INFORMATION | windows.PROCESS_VM_READ)
	h, err := windows.OpenProcess(access, false, uint32(pid))
	if err != nil {
		// Retry with limited query access if VM_READ is denied (still yields handle/GDI/times).
		h, err = windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, uint32(pid))
		if err != nil {
			return ProcessSample{}, fmt.Errorf("OpenProcess(%d): %w", pid, err)
		}
	}
	defer func() { _ = windows.CloseHandle(h) }()

	// Verify process has not already exited.
	var exitCode uint32
	if err := windows.GetExitCodeProcess(h, &exitCode); err == nil && exitCode != 259 { // STILL_ACTIVE = 259
		return ProcessSample{}, fmt.Errorf("process %d exited with code %d", pid, exitCode)
	}

	ps := ProcessSample{
		PID:   pid,
		Alive: true,
	}

	if err := readWindowsMemory(h, &ps); err != nil {
		return ProcessSample{}, fmt.Errorf("readWindowsMemory(%d): %w", pid, err)
	}
	ps.PrimaryMemoryBytes = ps.PrivateWorkingSetBytes

	var creation, exit, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(h, &creation, &exit, &kernel, &user); err == nil {
		ps.CPUUserNS = filetimeToNS(user)
		ps.CPUSystemNS = filetimeToNS(kernel)
		ps.CPUTotalNS = ps.CPUUserNS + ps.CPUSystemNS
	}

	var handles uint32
	if r1, _, _ := procGetProcessHandleCount.Call(uintptr(h), uintptr(unsafe.Pointer(&handles))); r1 != 0 {
		ps.Handles = handles
	}
	if r1, _, _ := procGetGuiResources.Call(uintptr(h), grGDIObjects); r1 != 0 {
		ps.GDIObjects = uint32(r1)
	}
	if r1, _, _ := procGetGuiResources.Call(uintptr(h), grUSERObjects); r1 != 0 {
		ps.USERObjects = uint32(r1)
	}
	if threads, err := countWindowsThreads(uint32(pid)); err == nil {
		ps.Threads = threads
	}

	vis, fg := countProcessWindows(uint32(pid))
	ps.VisibleWindows = vis
	ps.ForegroundWindows = fg

	return ps, nil
}

func readWindowsMemory(h windows.Handle, ps *ProcessSample) error {
	var ex2 processMemoryCountersEx2
	ex2.cb = uint32(unsafe.Sizeof(ex2))
	if r1, _, _ := procGetProcessMemoryInfo.Call(uintptr(h), uintptr(unsafe.Pointer(&ex2)), uintptr(ex2.cb)); r1 != 0 && ex2.privateWorkingSetSize > 0 {
		ps.PrivateWorkingSetBytes = uint64(ex2.privateWorkingSetSize)
		ps.PrivateCommitBytes = uint64(ex2.privateUsage)
		ps.WorkingSetBytes = uint64(ex2.workingSetSize)
		ps.PeakWorkingSetBytes = uint64(ex2.peakWorkingSetSize)
		return nil
	}

	var ex processMemoryCountersEx
	ex.cb = uint32(unsafe.Sizeof(ex))
	if r1, _, callErr := procGetProcessMemoryInfo.Call(uintptr(h), uintptr(unsafe.Pointer(&ex)), uintptr(ex.cb)); r1 == 0 {
		return fmt.Errorf("GetProcessMemoryInfo: %w", callErr)
	}
	ps.PrivateCommitBytes = uint64(ex.privateUsage)
	ps.WorkingSetBytes = uint64(ex.workingSetSize)
	ps.PeakWorkingSetBytes = uint64(ex.peakWorkingSetSize)

	privWS, err := queryWorkingSetPrivateBytes(h)
	if err == nil {
		ps.PrivateWorkingSetBytes = privWS
	}
	return nil
}

func queryWorkingSetPrivateBytes(h windows.Handle) (uint64, error) {
	pageSize := uint64(os.Getpagesize())
	if pageSize == 0 {
		pageSize = 4096
	}
	// Grow buffer if ERROR_BAD_LENGTH (24) is returned.
	nEntries := 65536
	for attempt := 0; attempt < 6; attempt++ {
		buf := make([]uintptr, 1+nEntries)
		byteSize := len(buf) * int(unsafe.Sizeof(uintptr(0)))
		r1, _, callErr := procQueryWorkingSet.Call(uintptr(h), uintptr(unsafe.Pointer(&buf[0])), uintptr(byteSize))
		if r1 != 0 {
			count := int(buf[0])
			if count > len(buf)-1 {
				count = len(buf) - 1
			}
			var priv uint64
			for i := 0; i < count; i++ {
				flags := buf[1+i]
				if (flags & wsSharedBit) == 0 {
					priv += pageSize
				}
			}
			return priv, nil
		}
		if errno, ok := callErr.(windows.Errno); ok && errno == windows.ERROR_BAD_LENGTH {
			if int(buf[0]) > nEntries {
				nEntries = int(buf[0]) + 4096
			} else {
				nEntries *= 2
			}
			continue
		}
		return 0, fmt.Errorf("QueryWorkingSet: %w", callErr)
	}
	return 0, fmt.Errorf("QueryWorkingSet exceeded buffer attempts")
}

func countWindowsThreads(pid uint32) (int, error) {
	snap, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPTHREAD, 0)
	if err != nil {
		return 0, err
	}
	defer func() { _ = windows.CloseHandle(snap) }()

	var te windows.ThreadEntry32
	te.Size = uint32(unsafe.Sizeof(te))
	if err := windows.Thread32First(snap, &te); err != nil {
		return 0, err
	}
	count := 0
	for {
		if te.OwnerProcessID == pid {
			count++
		}
		if err := windows.Thread32Next(snap, &te); err != nil {
			break
		}
	}
	return count, nil
}

func countProcessWindows(pid uint32) (visible int, foreground int) {
	enumMu.Lock()
	defer enumMu.Unlock()

	fg, _, _ := procGetForegroundWindow.Call()
	enumTargetPID = pid
	enumForegroundHWND = fg
	enumVisibleCount = 0
	enumForegroundCount = 0

	cb := getEnumWindowsCallback()
	_, _, _ = procEnumWindows.Call(cb, 0)
	return enumVisibleCount, enumForegroundCount
}

func filetimeToNS(ft windows.Filetime) uint64 {
	ticks := (uint64(ft.HighDateTime) << 32) | uint64(ft.LowDateTime)
	return ticks * 100
}

func captureAttributionNative(_ context.Context, phase string, rolePIDs map[string]int) (*AttributionSnapshot, error) {
	snap := &AttributionSnapshot{
		Phase:         phase,
		Platform:      "windows",
		CapturedAtUTC: time.Now().UTC().Format(time.RFC3339Nano),
		Processes:     make(map[int]ProcessAttribution, len(rolePIDs)),
	}
	for role, pid := range rolePIDs {
		if pid <= 0 {
			continue
		}
		h, err := windows.OpenProcess(windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_VM_READ, false, uint32(pid))
		if err != nil {
			snap.Warnings = append(snap.Warnings, fmt.Sprintf("OpenProcess(%d): %v", pid, err))
			continue
		}
		pa, werr := queryVirtualRegions(h, pid, role)
		_ = windows.CloseHandle(h)
		if werr != nil {
			snap.Warnings = append(snap.Warnings, werr.Error())
			continue
		}
		snap.Processes[pid] = pa
	}
	return snap, nil
}

func queryVirtualRegions(h windows.Handle, pid int, role string) (ProcessAttribution, error) {
	pa := ProcessAttribution{
		PID:  pid,
		Role: role,
		Note: "VirtualQueryEx region commits; not GPU VRAM attribution",
	}
	var addr uintptr
	var mbi memoryBasicInformation
	mbiSize := unsafe.Sizeof(mbi)
	for {
		ret, _, _ := procVirtualQueryEx.Call(uintptr(h), addr, uintptr(unsafe.Pointer(&mbi)), mbiSize)
		if ret == 0 {
			break
		}
		switch mbi.State {
		case memCommit:
			switch mbi.Type {
			case memPrivate:
				pa.PrivateCommittedBytes += uint64(mbi.RegionSize)
			case memMapped:
				pa.MappedCommittedBytes += uint64(mbi.RegionSize)
			case memImage:
				pa.ImageCommittedBytes += uint64(mbi.RegionSize)
			}
		case memReserve:
			pa.ReservedBytes += uint64(mbi.RegionSize)
		}
		next := mbi.BaseAddress + mbi.RegionSize
		if next <= addr {
			break
		}
		addr = next
	}
	return pa, nil
}

func clipboardSequenceNative() (uint64, error) {
	r1, _, _ := procGetClipboardSequenceNum.Call()
	return uint64(r1), nil
}

func hostArchNative() string {
	var procMachine, nativeMachine uint16
	h := windows.CurrentProcess()
	if r1, _, _ := procIsWow64Process2.Call(uintptr(h), uintptr(unsafe.Pointer(&procMachine)), uintptr(unsafe.Pointer(&nativeMachine))); r1 != 0 && nativeMachine != 0 {
		arch, _ := peMachineArch(nativeMachine)
		return arch
	}
	if pa := os.Getenv("PROCESSOR_ARCHITEW6432"); pa != "" {
		switch pa {
		case "AMD64":
			return "amd64"
		case "ARM64":
			return "arm64"
		}
	}
	return runtime.GOARCH
}

func fillHostInfoNative(_ context.Context, info *HostInfo) {
	var ms memoryStatusEx
	ms.dwLength = uint32(unsafe.Sizeof(ms))
	if r1, _, _ := procGlobalMemoryStatusEx.Call(uintptr(unsafe.Pointer(&ms))); r1 != 0 {
		info.PhysicalMemoryBytes = ms.ullTotalPhys
	}
	if maj, min, build := windows.RtlGetNtVersionNumbers(); maj > 0 {
		info.OSVersion = fmt.Sprintf("%d.%d.%d", maj, min, build)
	}
	if cpu := os.Getenv("PROCESSOR_IDENTIFIER"); cpu != "" {
		info.CPUModel = cpu
	}
}
