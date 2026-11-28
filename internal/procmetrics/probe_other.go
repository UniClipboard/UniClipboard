//go:build !windows && !linux && !(darwin && !ios)

package procmetrics

import (
	"time"

	"golang.org/x/sys/unix"
)

func readPlatformSnapshot() Snapshot {
	snap := Snapshot{
		Monotonic:     time.Now(),
		GoHeapBytes:   GoHeapBytes(),
		MemoryStatus:  StatusUnsupported,
		WakeupsStatus: StatusUnsupported,
	}
	var ru unix.Rusage
	if err := unix.Getrusage(unix.RUSAGE_SELF, &ru); err != nil {
		snap.CPUStatus = StatusError
	} else {
		snap.CPUTime = float64(ru.Utime.Sec) + float64(ru.Utime.Usec)/1e6 +
			float64(ru.Stime.Sec) + float64(ru.Stime.Usec)/1e6
		snap.CPUStatus = StatusOK
	}
	return snap
}
