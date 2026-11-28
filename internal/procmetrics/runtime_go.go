package procmetrics

import "runtime/metrics"

// GoHeapBytes returns Go heap in-use bytes (objects + unused) without triggering STW.
func GoHeapBytes() uint64 {
	samples := []metrics.Sample{
		{Name: "/memory/classes/heap/objects:bytes"},
		{Name: "/memory/classes/heap/unused:bytes"},
	}
	metrics.Read(samples)
	var total uint64
	for _, s := range samples {
		if s.Value.Kind() == metrics.KindUint64 {
			total += s.Value.Uint64()
		}
	}
	return total
}
