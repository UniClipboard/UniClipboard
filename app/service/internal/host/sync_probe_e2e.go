//go:build unic_e2e

package host

import (
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/syncdiag"
)

// ProbeSyncCollection measures the collector used by the real metrics worker.
func (n *Node) ProbeSyncCollection() func() []int64 {
	var mu sync.Mutex
	var samples []int64
	n.diagnostics.SetSyncCollector(func(epoch uint64) ([]syncdiag.Record, bool) {
		start := time.Now()
		records, more := n.peer.Collect(epoch)
		ns := time.Since(start).Nanoseconds()
		mu.Lock()
		if len(samples) < 256 {
			samples = append(samples, ns)
		}
		mu.Unlock()
		return records, more
	})
	return func() []int64 {
		mu.Lock()
		defer mu.Unlock()
		return append([]int64{}, samples...)
	}
}
