//go:build bench_probe

package desktopui

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
	"weak"
)

// BenchCacheSnapshot holds aggregated, privacy-safe counts of UI projection and image cache objects.
type BenchCacheSnapshot struct {
	Entries           int    `json:"entries"`
	MetadataItems     int    `json:"metadata_items"`
	Details           int    `json:"details"`
	DetailWindow      int    `json:"detail_window"`
	DetailReads       int    `json:"detail_reads"`
	ImageOwners       int    `json:"image_owners"`
	SelectedOriginals int    `json:"selected_originals"`
	ReleasedImages    int    `json:"released_images"`
	Layout            string `json:"layout,omitempty"`
}

var (
	benchMu                sync.Mutex
	benchActiveView        weak.Pointer[viewState]
	benchRoleName          = "desktop"
	benchLastSelectedID    string
	benchLastSelectedReady bool
	benchEventSeq          uint64
	benchSelectionSeq      uint64
)

// SetBenchRole sets the role label used in bench_probe event filenames.
func SetBenchRole(role string) {
	benchMu.Lock()
	if role != "" {
		benchRoleName = role
	}
	benchMu.Unlock()
}

func benchRegisterView(d *viewState) {
	if d == nil {
		return
	}
	benchMu.Lock()
	benchActiveView = weak.Make(d)
	benchMu.Unlock()
}

func benchNotifyView(d *viewState) {
	if d == nil {
		return
	}
	benchMu.Lock()
	benchActiveView = weak.Make(d)
	role := benchRoleName
	selected := d.selected
	if selected == "" {
		benchLastSelectedID = ""
		benchLastSelectedReady = false
	} else if selected != benchLastSelectedID {
		benchLastSelectedID = selected
		benchLastSelectedReady = false
		benchSelectionSeq++
	}
	shouldEmit := false
	var w, h int
	var seq, selSeq uint64
	if selected != "" && !benchLastSelectedReady {
		if cur := d.current(); cur != nil && cur.picture != nil && cur.picture.edge == 0 && cur.picture.bitmap != nil {
			benchLastSelectedReady = true
			benchEventSeq++
			seq = benchEventSeq
			selSeq = benchSelectionSeq
			shouldEmit = true
			w, h = cur.picture.width, cur.picture.height
		}
	}
	benchMu.Unlock()

	if !shouldEmit {
		return
	}
	dir := os.Getenv("UNIC_BENCH_METRICS_DIR")
	if dir == "" {
		return
	}
	_ = os.MkdirAll(dir, 0o700)
	ev := map[string]any{
		"event":         "selected-original-ready",
		"role":          role,
		"pid":           os.Getpid(),
		"seq":           seq,
		"selection_seq": selSeq,
		"unix_millis":   time.Now().UTC().UnixMilli(),
		"width":         w,
		"height":        h,
	}
	raw, err := json.Marshal(ev)
	if err != nil {
		return
	}
	path := filepath.Join(dir, fmt.Sprintf("%s-%d.events.jsonl", role, os.Getpid()))
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return
	}
	_, _ = f.Write(append(raw, '\n'))
	_ = f.Close()
}

// ReadBenchCacheSnapshot reads aggregated cache object counts via a weak pointer
// without retaining released view states or image buffers. Call on the UI thread.
func ReadBenchCacheSnapshot() (BenchCacheSnapshot, bool) {
	benchMu.Lock()
	wp := benchActiveView
	benchMu.Unlock()

	d := wp.Value()
	if d == nil || d.live == nil {
		return BenchCacheSnapshot{}, false
	}
	l := d.live
	seen := make(map[*historyImage]struct{})
	add := func(e entry) {
		if e.picture != nil {
			seen[e.picture] = struct{}{}
		}
	}
	for _, e := range l.details {
		add(e)
	}
	for _, e := range d.entries {
		add(e)
	}
	if l.retained != nil {
		add(*l.retained)
	}
	originals := 0
	released := 0
	for p := range seen {
		if p.edge == 0 {
			originals++
		}
		if p.released {
			released++
		}
	}
	return BenchCacheSnapshot{
		Entries:           len(d.entries),
		MetadataItems:     len(l.items),
		Details:           len(l.details),
		DetailWindow:      len(l.detailWindow),
		DetailReads:       len(l.detailReads),
		ImageOwners:       len(seen),
		SelectedOriginals: originals,
		ReleasedImages:    released,
		Layout:            d.layout,
	}, true
}
