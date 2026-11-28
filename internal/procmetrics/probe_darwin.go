//go:build darwin && !ios

package procmetrics

import (
	"os"
	"sync"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

const rusageInfoV0 = 0

type rusageInfoV0Data struct {
	UUID             [16]byte
	UserTime         uint64
	SystemTime       uint64
	PkgIdleWkups     uint64
	InterruptWkups   uint64
	Pageins          uint64
	WiredSize        uint64
	ResidentSize     uint64
	PhysFootprint    uint64
	ProcStartAbstime uint64
	ProcExitAbstime  uint64
}

type machTimebaseInfoData struct {
	Numer uint32
	Denom uint32
}

var darwinOnce sync.Once
var (
	darwinProcPIDRusage      func(pid int32, flavor int32, buffer unsafe.Pointer) int32
	darwinMachContinuousTime func() uint64
	darwinTimebase           machTimebaseInfoData
	darwinInitErr            error
)

func initDarwinProbes() {
	darwinOnce.Do(func() {
		lib, err := purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			darwinInitErr = err
			return
		}
		purego.RegisterLibFunc(&darwinProcPIDRusage, lib, "proc_pid_rusage")
		purego.RegisterLibFunc(&darwinMachContinuousTime, lib, "mach_continuous_time")
		var timebaseFn func(*machTimebaseInfoData) int32
		purego.RegisterLibFunc(&timebaseFn, lib, "mach_timebase_info")
		if timebaseFn(&darwinTimebase) != 0 || darwinTimebase.Denom == 0 {
			darwinTimebase = machTimebaseInfoData{Numer: 1, Denom: 1}
		}
	})
}

func darwinBootElapsed() time.Duration {
	initDarwinProbes()
	if darwinInitErr != nil || darwinMachContinuousTime == nil || darwinTimebase.Denom == 0 {
		return 0
	}
	ticks := darwinMachContinuousTime()
	// Compute (ticks * numer) / denom safely without uint64 overflow.
	numer := uint64(darwinTimebase.Numer)
	denom := uint64(darwinTimebase.Denom)
	ns := (ticks/denom)*numer + ((ticks%denom)*numer)/denom
	return time.Duration(ns)
}

func readPlatformSnapshot() Snapshot {
	snap := Snapshot{
		Monotonic:   time.Now(),
		BootElapsed: darwinBootElapsed(),
		GoHeapBytes: GoHeapBytes(),
	}

	var ru unix.Rusage
	if err := unix.Getrusage(unix.RUSAGE_SELF, &ru); err != nil {
		snap.CPUStatus = StatusError
	} else {
		snap.CPUTime = float64(ru.Utime.Sec) + float64(ru.Utime.Usec)/1e6 +
			float64(ru.Stime.Sec) + float64(ru.Stime.Usec)/1e6
		snap.CPUStatus = StatusOK
	}

	initDarwinProbes()
	if darwinInitErr != nil || darwinProcPIDRusage == nil {
		snap.MemoryStatus = StatusError
		snap.WakeupsStatus = StatusError
		return snap
	}

	var info rusageInfoV0Data
	rc := darwinProcPIDRusage(int32(os.Getpid()), rusageInfoV0, unsafe.Pointer(&info))
	if rc != 0 {
		snap.MemoryStatus = StatusError
		snap.WakeupsStatus = StatusError
		return snap
	}
	if info.PhysFootprint == 0 {
		snap.MemoryStatus = StatusError
	} else {
		snap.MemoryFootprintBytes = info.PhysFootprint
		snap.MemoryStatus = StatusOK
	}
	snap.Wakeups = info.InterruptWkups
	snap.WakeupsStatus = StatusOK
	return snap
}
