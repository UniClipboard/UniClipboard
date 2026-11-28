//go:build bench_probe

package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime/metrics"
	"sync"
	"time"
)

type unicdBenchRuntimeSample struct {
	UTCMillis    int64   `json:"utc_millis"`
	ElapsedSec   float64 `json:"elapsed_sec"`
	Role         string  `json:"role"`
	PID          int     `json:"pid"`
	HeapAlloc    uint64  `json:"heap_alloc"`
	HeapInuse    uint64  `json:"heap_inuse"`
	HeapSys      uint64  `json:"heap_sys"`
	HeapReleased uint64  `json:"heap_released"`
	StackInuse   uint64  `json:"stack_inuse"`
	NumGC        uint64  `json:"num_gc"`
	Goroutines   uint64  `json:"goroutines"`
}

func init() {
	dir := os.Getenv("UNIC_BENCH_METRICS_DIR")
	if dir == "" {
		return
	}
	go runUnicdBenchProbe(dir)
}

func runUnicdBenchProbe(dir string) {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return
	}
	path := filepath.Join(dir, fmt.Sprintf("unicd-%d.runtime.jsonl", os.Getpid()))
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return
	}
	defer func() { _ = f.Close() }()

	start := time.Now()
	var mu sync.Mutex
	writeSample := func() {
		s := readUnicdRuntimeMetrics(start)
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

func readUnicdRuntimeMetrics(start time.Time) unicdBenchRuntimeSample {
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

	return unicdBenchRuntimeSample{
		UTCMillis:    time.Now().UTC().UnixMilli(),
		ElapsedSec:   time.Since(start).Seconds(),
		Role:         "unicd",
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
