package observability

import (
	"encoding/json"
	"io"
	"os"
	"time"

	"github.com/uniclipboard/unic/engine"
)

type dailyMarker struct {
	Identity string `json:"identity"`
	Day      string `json:"day"`
}

type dailySlot struct {
	markerFile string
	tempFile   string
	event      engine.ProductEvent
	marked     string
	pending    string
	failed     bool
}

const (
	dailySlotRemote = 0
	dailySlotActive = 1

	dailyMarkerFile          = "analytics-remote-day"
	dailyTemporaryFile       = ".analytics-remote-day.tmp"
	dailyActiveMarkerFile    = "analytics-active-day"
	dailyActiveTemporaryFile = ".analytics-active-day.tmp"
)

func (c *Controller) now() time.Time {
	if c.opts.Now != nil {
		return c.opts.Now()
	}
	return time.Now()
}

func (g *analyticsGeneration) loadDaily() {
	g.dailyWake = make(chan struct{}, 1)
	g.dailyStop = make(chan struct{})
	g.dailyDone = make(chan struct{})
	g.daily = [2]dailySlot{
		dailySlotRemote: {
			markerFile: dailyMarkerFile,
			tempFile:   dailyTemporaryFile,
			event:      engine.ProductEvent{Name: "remote_content_applied", Result: "succeeded"},
		},
		dailySlotActive: {
			markerFile: dailyActiveMarkerFile,
			tempFile:   dailyActiveTemporaryFile,
			event:      engine.ProductEvent{Name: "daily_active"},
		},
	}
	root, err := os.OpenRoot(g.c.opts.Dir)
	if err != nil {
		for i := range g.daily {
			g.daily[i].failed = true
		}
		g.c.drop(3, 3, 1)
		return
	}
	defer func() { _ = root.Close() }()
	for i := range g.daily {
		g.loadDailySlot(root, &g.daily[i])
	}
}

func (g *analyticsGeneration) loadDailySlot(root *os.Root, slot *dailySlot) {
	// The fixed reserved temporary name bounds crash residue to one file. The
	// exclusive profile owner removes a stale temporary before daily admission.
	if err := root.Remove(slot.tempFile); err != nil && !os.IsNotExist(err) {
		slot.failed = true
		g.c.drop(3, 3, 1)
		return
	}
	info, err := root.Lstat(slot.markerFile)
	if os.IsNotExist(err) {
		return
	}
	if err != nil || !info.Mode().IsRegular() {
		slot.failed = true
		g.c.drop(3, 3, 1)
		return
	}
	f, err := root.Open(slot.markerFile)
	if err != nil {
		slot.failed = true
		g.c.drop(3, 3, 1)
		return
	}
	defer func() { _ = f.Close() }()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(info, actual) {
		slot.failed = true
		g.c.drop(3, 3, 1)
		return
	}
	var marker dailyMarker
	data, err := io.ReadAll(io.LimitReader(f, 256))
	if err != nil || json.Unmarshal(data, &marker) != nil || !validDay(marker.Day) {
		slot.failed = true
		g.c.drop(3, 3, 1)
		return
	}
	if marker.Identity == g.identity {
		slot.marked = marker.Day
	}
}

func validDay(day string) bool {
	v, err := time.Parse("2006-01-02", day)
	return err == nil && v.Format("2006-01-02") == day
}

func (g *analyticsGeneration) notifyDaily(slotIdx int) {
	day := g.c.now().UTC().Format("2006-01-02")
	g.mu.Lock()
	defer g.mu.Unlock()
	slot := &g.daily[slotIdx]
	if !g.active || slot.failed || day <= slot.marked || day <= slot.pending {
		return
	}
	slot.pending = day
	select {
	case g.dailyWake <- struct{}{}:
	default:
	}
}

func (g *analyticsGeneration) dailyWorker() {
	defer close(g.dailyDone)
	for {
		select {
		case <-g.dailyStop:
			return
		case <-g.dailyWake:
			for i := range g.daily {
				g.mu.Lock()
				slot := &g.daily[i]
				day := slot.pending
				slot.pending = ""
				admit := g.active && !slot.failed && day > slot.marked
				if admit {
					slot.marked = day
				}
				markerFile, tempFile, ev := slot.markerFile, slot.tempFile, slot.event
				g.mu.Unlock()
				if !admit {
					continue
				}
				if err := g.persistDailySlot(markerFile, tempFile, day); err != nil {
					g.c.drop(3, 3, 1)
					continue
				}
				// Persistence precedes enqueue. Loss never causes same-day replay.
				g.enqueue(ev)
			}
		}
	}
}

func (g *analyticsGeneration) persistDailySlot(markerFile, tempFile, day string) error {
	root, err := os.OpenRoot(g.c.opts.Dir)
	if err != nil {
		return err
	}
	defer func() { _ = root.Close() }()
	f, err := root.OpenFile(tempFile, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	defer func() { _ = root.Remove(tempFile) }()
	err = json.NewEncoder(f).Encode(dailyMarker{Identity: g.identity, Day: day})
	if err == nil {
		err = f.Sync()
	}
	cerr := f.Close()
	if err == nil {
		err = cerr
	}
	if err == nil {
		err = root.Rename(tempFile, markerFile)
	}
	return err
}

func clearDailyMarker(dir string) error {
	root, err := os.OpenRoot(dir)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return err
	}
	defer func() { _ = root.Close() }()
	var failure error
	for _, name := range []string{dailyTemporaryFile, dailyMarkerFile, dailyActiveTemporaryFile, dailyActiveMarkerFile} {
		if err := root.Remove(name); err != nil && !os.IsNotExist(err) && failure == nil {
			failure = err
		}
	}
	return failure
}
