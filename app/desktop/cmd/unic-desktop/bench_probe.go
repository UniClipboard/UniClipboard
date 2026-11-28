//go:build bench_probe

package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime/metrics"
	"sync"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
)

type desktopBenchRuntimeSample struct {
	UTCMillis    int64                         `json:"utc_millis"`
	ElapsedSec   float64                       `json:"elapsed_sec"`
	Role         string                        `json:"role"`
	PID          int                           `json:"pid"`
	HeapAlloc    uint64                        `json:"heap_alloc"`
	HeapInuse    uint64                        `json:"heap_inuse"`
	HeapSys      uint64                        `json:"heap_sys"`
	HeapReleased uint64                        `json:"heap_released"`
	StackInuse   uint64                        `json:"stack_inuse"`
	NumGC        uint64                        `json:"num_gc"`
	Goroutines   uint64                        `json:"goroutines"`
	Cache        *desktopui.BenchCacheSnapshot `json:"cache,omitempty"`
}

func init() {
	dir := os.Getenv("UNIC_BENCH_METRICS_DIR")
	if dir == "" {
		return
	}
	role := detectBenchRole(os.Args[1:])
	if role == "command" {
		return
	}
	desktopui.SetBenchRole(role)
	go runDesktopBenchProbe(dir, role)
}

func detectBenchRole(args []string) string {
	role := "launcher"
	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--command":
			return "command"
		case "--quick-panel":
			return "quick-panel"
		case "--role":
			if i+1 < len(args) && args[i+1] != "" {
				role = args[i+1]
			}
		}
	}
	return role
}

func runDesktopBenchProbe(dir, role string) {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return
	}
	path := filepath.Join(dir, fmt.Sprintf("%s-%d.runtime.jsonl", role, os.Getpid()))
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return
	}
	defer func() { _ = f.Close() }()

	start := time.Now()
	var mu sync.Mutex
	writeSample := func() {
		s := readDesktopRuntimeMetrics(start, role)
		if role == "gui" || role == "quick-panel" {
			if snap, ok := sampleCacheOnMain(); ok {
				s.Cache = &snap
			}
		}
		raw, merr := json.Marshal(s)
		if merr != nil {
			return
		}
		mu.Lock()
		_, _ = f.Write(append(raw, '\n'))
		mu.Unlock()
	}

	writeSample()
	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()
	for range ticker.C {
		writeSample()
	}
}

type cacheSampleResult struct {
	snap desktopui.BenchCacheSnapshot
	ok   bool
}

var cacheSampleInFlight atomic.Bool

func sampleCacheOnMain() (desktopui.BenchCacheSnapshot, bool) {
	if !cacheSampleInFlight.CompareAndSwap(false, true) {
		return desktopui.BenchCacheSnapshot{}, false
	}
	resCh := make(chan cacheSampleResult, 1)
	go func() {
		defer cacheSampleInFlight.Store(false)
		mygo.RunOnMain(func() {
			snap, ok := desktopui.ReadBenchCacheSnapshot()
			resCh <- cacheSampleResult{snap: snap, ok: ok}
		})
	}()
	timer := time.NewTimer(500 * time.Millisecond)
	defer timer.Stop()
	select {
	case res := <-resCh:
		return res.snap, res.ok
	case <-timer.C:
		return desktopui.BenchCacheSnapshot{}, false
	}
}

func readDesktopRuntimeMetrics(start time.Time, role string) desktopBenchRuntimeSample {
	samples := []metrics.Sample{
		{Name: "/memory/classes/heap/objects:bytes"},
		{Name: "/memory/classes/heap/unused:bytes"},
		{Name: "/memory/classes/heap/free:bytes"},
		{Name: "/memory/classes/heap/released:bytes"},
		{Name: "/memory/classes/heap/stacks:bytes"},
		{Name: "/gc/cycles/total:gc-cycles"},
		{Name: "/sched/goroutines:goroutines"},
	}
	metrics.Read(samples)

	get := func(i int) uint64 {
		if samples[i].Value.Kind() == metrics.KindUint64 {
			return samples[i].Value.Uint64()
		}
		return 0
	}

	heapObjects := get(0)
	heapUnused := get(1)
	heapFree := get(2)
	heapReleased := get(3)
	stackInuse := get(4)
	numGC := get(5)
	goroutines := get(6)

	return desktopBenchRuntimeSample{
		UTCMillis:    time.Now().UTC().UnixMilli(),
		ElapsedSec:   time.Since(start).Seconds(),
		Role:         role,
		PID:          os.Getpid(),
		HeapAlloc:    heapObjects,
		HeapInuse:    heapObjects + heapUnused,
		HeapSys:      heapObjects + heapUnused + heapFree + heapReleased,
		HeapReleased: heapReleased,
		StackInuse:   stackInuse,
		NumGC:        numGC,
		Goroutines:   goroutines,
	}
}
