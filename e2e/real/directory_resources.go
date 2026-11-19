package main

import (
	"context"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"
)

type directoryOpenFile struct {
	Class     string `json:"class"`
	Identity  string `json:"identity"`
	Size      int64  `json:"size"`
	Directory bool   `json:"directory"`
	Deleted   bool   `json:"deleted"`
}
type directoryResources struct {
	StageFiles           []directoryOpenFile `json:"staging_or_fixture_descriptors,omitempty"`
	RSSBytes             int64               `json:"rss_bytes"`
	OpenFiles            int                 `json:"open_files"`
	StageOpenFiles       int                 `json:"open_staging_or_fixture_files"`
	IncomingFiles        int                 `json:"incoming_files"`
	IncomingBytes        int64               `json:"incoming_bytes"`
	TransientDirectories int                 `json:"transient_directories"`
}
type directoryMeasure struct {
	Before       directoryResources `json:"before"`
	After        directoryResources `json:"after"`
	PeakRSSBytes int64              `json:"sampled_peak_rss_bytes"`
	Samples      int                `json:"samples"`
	Stages       map[string]float64 `json:"first_stage_observed_ms,omitempty"`
}

func (w *worker) directoryResources() directoryResources {
	r := directoryResources{OpenFiles: -1, StageOpenFiles: -1}
	if w.daemon != nil && w.daemon.Process != nil {
		pid := strconv.Itoa(w.daemon.Process.Pid)
		if runtime.GOOS != "windows" {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			b, err := exec.CommandContext(ctx, "ps", "-o", "rss=", "-p", pid).Output()
			cancel()
			if err == nil {
				n, _ := strconv.ParseInt(strings.TrimSpace(string(b)), 10, 64)
				r.RSSBytes = n * 1024
			}
		}
		if runtime.GOOS == "linux" {
			entries, err := os.ReadDir("/proc/" + pid + "/fd")
			if err == nil {
				r.OpenFiles = len(entries)
				r.StageOpenFiles = 0
				for _, entry := range entries {
					target, e := os.Readlink("/proc/" + pid + "/fd/" + entry.Name())
					if e == nil && (strings.HasPrefix(target, filepath.Join(w.root, "fixtures")+string(filepath.Separator)) || strings.HasPrefix(target, filepath.Join(w.root, "state", "inbox", "unic-recv")+string(filepath.Separator)) || strings.Contains(target, filepath.Join(w.root, "state", "inbox", "unic-content-"))) {
						r.StageOpenFiles++
						class := "transient"
						if strings.HasPrefix(target, filepath.Join(w.root, "fixtures")) {
							class = "fixture"
						} else if strings.Contains(target, "unic-recv") {
							class = "incoming"
						}
						detail := directoryOpenFile{Class: class, Identity: digest([]byte(strings.TrimPrefix(target, w.root)))[:16], Deleted: strings.HasSuffix(target, " (deleted)")}
						if st, e := os.Stat("/proc/" + pid + "/fd/" + entry.Name()); e == nil {
							detail.Size = st.Size()
							detail.Directory = st.IsDir()
						}
						r.StageFiles = append(r.StageFiles, detail)
					}
				}
			}
		}
	}
	inbox := filepath.Join(w.root, "state", "inbox")
	entries, _ := os.ReadDir(inbox)
	for _, e := range entries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "unic-content-") {
			r.TransientDirectories++
		}
	}
	_ = filepath.WalkDir(filepath.Join(inbox, "unic-recv"), func(p string, d fs.DirEntry, err error) error {
		if err == nil && d.Type().IsRegular() {
			if info, e := d.Info(); e == nil {
				r.IncomingFiles++
				r.IncomingBytes += info.Size()
			}
		}
		return nil
	})
	return r
}
func (w *worker) monitorDirectory(ctx context.Context, key string) func() directoryMeasure {
	before := w.directoryResources()
	started := time.Now()
	stop, done := make(chan struct{}), make(chan directoryMeasure, 1)
	go func() {
		m := directoryMeasure{Before: before, PeakRSSBytes: before.RSSBytes, Stages: map[string]float64{}}
		ticker := time.NewTicker(100 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-stop:
				m.After = w.directoryResources()
				if m.After.RSSBytes > m.PeakRSSBytes {
					m.PeakRSSBytes = m.After.RSSBytes
				}
				done <- m
				return
			case <-ticker.C:
				r := w.directoryResources()
				m.Samples++
				if r.RSSBytes > m.PeakRSSBytes {
					m.PeakRSSBytes = r.RSSBytes
				}
				if key != "" {
					probe, cancel := context.WithTimeout(ctx, time.Second)
					s, err := w.client.Submission(probe, key)
					cancel()
					if err == nil && s.Stage != "" {
						if _, ok := m.Stages[s.Stage]; !ok {
							m.Stages[s.Stage] = float64(time.Since(started)) / float64(time.Millisecond)
						}
					}
				}
			}
		}
	}()
	return func() directoryMeasure { close(stop); return <-done }
}

func (w *worker) submit(ctx context.Context, v submitRequest) (submitResult, error) {
	f, ok := w.fixtures[v.Fixture]
	if !ok || f.Meta.Kind != "directory" {
		return w.submitContent(ctx, v)
	}
	finish := w.monitorDirectory(ctx, v.Key)
	out, err := w.submitContent(ctx, v)
	m := finish()
	out.Resources = &m
	return out, err
}
func (w *worker) wait(ctx context.Context) (delivery, error) {
	if w.gate == nil || w.gate.Fixture.Kind != "directory" {
		return w.waitContent(ctx)
	}
	finish := w.monitorDirectory(ctx, "")
	out, err := w.waitContent(ctx)
	m := finish()
	out.Resources = &m
	return out, err
}
