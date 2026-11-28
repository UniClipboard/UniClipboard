//go:build linux

package procmetrics

import (
	"io"
	"os"
	"time"

	"golang.org/x/sys/unix"
)

const maxProcReadBytes = 64 << 10

func readProcBounded(path string) ([]byte, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer func() { _ = f.Close() }()
	return io.ReadAll(io.LimitReader(f, maxProcReadBytes))
}

func linuxBootElapsed() time.Duration {
	var ts unix.Timespec
	if err := unix.ClockGettime(unix.CLOCK_BOOTTIME, &ts); err != nil {
		return 0
	}
	return time.Duration(ts.Sec)*time.Second + time.Duration(ts.Nsec)
}

func readPlatformSnapshot() Snapshot {
	snap := Snapshot{
		Monotonic:   time.Now(),
		BootElapsed: linuxBootElapsed(),
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

	statusBytes, statusErr := readProcBounded("/proc/self/status")

	if rollupBytes, err := readProcBounded("/proc/self/smaps_rollup"); err == nil {
		if pss, ok := ParseProcField(rollupBytes, "Pss", 1024); ok && pss > 0 {
			snap.MemoryFootprintBytes = pss
			snap.MemoryStatus = StatusOK
		}
	}
	if snap.MemoryStatus != StatusOK && statusErr == nil {
		if rss, ok := ParseProcField(statusBytes, "VmRSS", 1024); ok && rss > 0 {
			snap.MemoryFootprintBytes = rss
			snap.MemoryStatus = StatusOK
		}
	}
	if snap.MemoryStatus != StatusOK {
		snap.MemoryStatus = StatusError
	}

	if statusErr == nil {
		if csw, ok := ParseProcField(statusBytes, "voluntary_ctxt_switches", 1); ok {
			snap.ContextSwitches = csw
			snap.ContextSwitchesStatus = StatusOK
		} else {
			snap.ContextSwitchesStatus = StatusError
		}
	} else {
		snap.ContextSwitchesStatus = StatusError
	}

	return snap
}
