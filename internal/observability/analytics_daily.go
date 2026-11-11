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

const dailyMarkerFile = "analytics-remote-day"
const dailyTemporaryFile = ".analytics-remote-day.tmp"

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
	root, err := os.OpenRoot(g.c.opts.Dir)
	if err != nil {
		g.dailyFailed = true
		g.c.drop(3, 3, 1)
		return
	}
	defer root.Close()
	// The fixed reserved temporary name bounds crash residue to one file. The
	// exclusive profile owner removes a stale temporary before daily admission.
	if err := root.Remove(dailyTemporaryFile); err != nil && !os.IsNotExist(err) {
		g.dailyFailed = true
		g.c.drop(3, 3, 1)
		return
	}
	info, err := root.Lstat(dailyMarkerFile)
	if os.IsNotExist(err) {
		return
	}
	if err != nil || !info.Mode().IsRegular() {
		g.dailyFailed = true
		g.c.drop(3, 3, 1)
		return
	}
	f, err := root.Open(dailyMarkerFile)
	if err != nil {
		g.dailyFailed = true
		g.c.drop(3, 3, 1)
		return
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(info, actual) {
		g.dailyFailed = true
		g.c.drop(3, 3, 1)
		return
	}
	var marker dailyMarker
	data, err := io.ReadAll(io.LimitReader(f, 256))
	if err != nil || json.Unmarshal(data, &marker) != nil || !validDay(marker.Day) {
		g.dailyFailed = true
		g.c.drop(3, 3, 1)
		return
	}
	if marker.Identity == g.identity {
		g.dailyMarked = marker.Day
	}
}
func validDay(day string) bool {
	v, err := time.Parse("2006-01-02", day)
	return err == nil && v.Format("2006-01-02") == day
}
func (g *analyticsGeneration) notifyDaily() {
	day := g.c.now().UTC().Format("2006-01-02")
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active || g.dailyFailed || day <= g.dailyMarked || day <= g.dailyPending {
		return
	}
	g.dailyPending = day
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
			g.mu.Lock()
			day := g.dailyPending
			g.dailyPending = ""
			admit := g.active && !g.dailyFailed && day > g.dailyMarked
			if admit {
				g.dailyMarked = day
			}
			g.mu.Unlock()
			if !admit {
				continue
			}
			if err := g.persistDaily(day); err != nil {
				g.c.drop(3, 3, 1)
				continue
			}
			// Persistence precedes enqueue. Loss never causes same-day replay.
			g.enqueue(engine.ProductEvent{Name: "remote_content_applied", Result: "succeeded"})
		}
	}
}
func (g *analyticsGeneration) persistDaily(day string) error {
	root, err := os.OpenRoot(g.c.opts.Dir)
	if err != nil {
		return err
	}
	defer root.Close()
	temp := dailyTemporaryFile
	f, err := root.OpenFile(temp, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	defer root.Remove(temp)
	err = json.NewEncoder(f).Encode(dailyMarker{Identity: g.identity, Day: day})
	if err == nil {
		err = f.Sync()
	}
	cerr := f.Close()
	if err == nil {
		err = cerr
	}
	if err == nil {
		err = root.Rename(temp, dailyMarkerFile)
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
	defer root.Close()
	var failure error
	for _, name := range []string{dailyTemporaryFile, dailyMarkerFile} {
		if err := root.Remove(name); err != nil && !os.IsNotExist(err) && failure == nil {
			failure = err
		}
	}
	return failure
}
