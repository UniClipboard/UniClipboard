package observability

import (
	"context"
	"encoding/json"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
)

var events = map[string]bool{"engine.created": true, "engine.started": true, "engine.stopped": true, "engine.closed": true, "pipeline.completed": true, "pipeline.failed": true, "host.failed": true, "config.reloaded": true, "config.rejected": true, "diagnostics.changed": true, "telemetry.degraded": true}
var results = map[string]bool{"accepted": true, "baseline": true, "loop_prevented": true, "policy_filtered": true, "failed": true, "succeeded": true, "cancelled": true, "started": true, "superseded": true, "duplicate": true, "rejected": true, "deferred": true}

func safeAttr(a slog.Attr) (slog.Attr, bool) {
	if a.Value.Kind() != slog.KindString {
		return slog.Attr{}, false
	}
	v := a.Value.String()
	if len(v) > 256 {
		return slog.Attr{}, false
	}
	ok := false
	switch a.Key {
	case "stage":
		ok = stages[v] != nil
	case "result":
		ok = results[v]
	case "backend":
		ok = v == "none" || v == "tailcat" || v == "external"
	case "content_kind":
		ok = v == "text" || v == "image" || v == "files" || v == "mixed" || v == "unknown"
	case "code":
		ok = validCode(v) || v == "dns" || v == "tls" || v == "unauthorized" || v == "initialization_failed" || v == "restart_required"
	case "target":
		ok = v == "local" || v == "remote"
	case "source":
		ok = v == "config" || v == "override"
	}
	return a, ok
}

type handler struct {
	c       *Controller
	attrs   []slog.Attr
	grouped bool
}

func (h *handler) Enabled(_ context.Context, l slog.Level) bool {
	c := h.c
	c.mu.Lock()
	expired := c.expireLevelsLocked()
	local, _ := Level(c.levelLocked(0).Effective)
	remote, _ := Level(c.levelLocked(1).Effective)
	enabled := !c.closed && (l >= local || c.logs != nil && l >= remote)
	c.mu.Unlock()
	if expired {
		c.changed()
	}
	return enabled
}

func (h *handler) WithAttrs(attrs []slog.Attr) slog.Handler {
	n := &handler{c: h.c, attrs: append([]slog.Attr{}, h.attrs...), grouped: h.grouped}
	if !h.grouped {
		for _, a := range attrs {
			if a, ok := safeAttr(a); ok {
				n.attrs = append(n.attrs, a)
			}
		}
	}
	if len(n.attrs) > 7 {
		n.attrs = n.attrs[len(n.attrs)-7:]
	}
	return n
}
func (h *handler) WithGroup(name string) slog.Handler {
	n := *h
	if name != "" {
		n.grouped = true
	}
	return &n
}
func (h *handler) Handle(_ context.Context, r slog.Record) error {
	if !events[r.Message] {
		return nil
	}
	fields := make(map[string]string, 7)
	for _, a := range h.attrs {
		fields[a.Key] = a.Value.String()
	}
	if !h.grouped {
		r.Attrs(func(a slog.Attr) bool {
			if a, ok := safeAttr(a); ok {
				fields[a.Key] = a.Value.String()
			}
			return true
		})
	}
	when := r.Time
	if when.IsZero() {
		when = time.Now()
	}
	when = when.UTC()
	clean := slog.NewRecord(when, r.Level, r.Message, 0)
	obj := make(map[string]any, len(fields)+3)
	obj["time"] = when.Format(time.RFC3339Nano)
	obj["level"] = r.Level.String()
	obj["msg"] = r.Message
	for k, v := range fields {
		obj[k] = v
		clean.AddAttrs(slog.String(k, v))
	}
	encoded, err := json.Marshal(obj)
	if err != nil {
		return nil
	}
	encoded = append(encoded, '\n')
	c := h.c
	c.mu.Lock()
	expired := c.expireLevelsLocked()
	if c.closed {
		c.mu.Unlock()
		return nil
	}
	local, _ := Level(c.levelLocked(0).Effective)
	remote, _ := Level(c.levelLocked(1).Effective)
	g := c.logs
	if r.Level >= local {
		if len(encoded) > 8192 {
			c.states[0].Dropped++
			c.drops[0][1]++
		} else {
			select {
			case c.local <- encoded:
			default:
				c.states[0].Dropped++
				c.drops[0][0]++
			}
		}
	}
	c.mu.Unlock()
	if expired {
		c.changed()
	}
	if g != nil && r.Level >= remote {
		if len(encoded) > 8192 {
			c.drop(1, 1, 1)
		} else {
			_ = g.handler.Handle(context.Background(), clean)
		}
	}
	return nil
}
func (c *Controller) localDegraded(code string) {
	// Called only on state transitions. A remote exporter never receives this.
	obj := map[string]string{"time": time.Now().UTC().Format(time.RFC3339Nano), "level": "WARN", "msg": "telemetry.degraded", "code": code}
	b, _ := json.Marshal(obj)
	b = append(b, '\n')
	c.mu.Lock()
	expired := c.expireLevelsLocked()
	defer func() {
		if expired {
			c.changed()
		}
	}()
	defer c.mu.Unlock()
	level, _ := Level(c.levelLocked(0).Effective)
	if c.closed || slog.LevelWarn < level || time.Since(c.lastLocalError) < time.Minute {
		return
	}
	c.lastLocalError = time.Now()
	select {
	case c.local <- b:
	default:
		c.states[0].Dropped++
		c.drops[0][0]++
	}
}
func (c *Controller) localWorker() {
	defer close(c.localDone)
	base, err := os.OpenRoot(c.opts.Dir)
	if err != nil {
		c.drainLocal(nil)
		return
	}
	defer func() { _ = base.Close() }()
	if err = base.Mkdir("logs", 0700); err != nil && !os.IsExist(err) {
		c.drainLocal(nil)
		return
	}
	st, err := base.Lstat("logs")
	if err != nil || !st.IsDir() {
		c.drainLocal(nil)
		return
	}
	if err := base.Chmod("logs", 0700); err != nil {
		c.drainLocal(nil)
		return
	}
	root, err := base.OpenRoot("logs")
	if err != nil {
		c.drainLocal(nil)
		return
	}
	defer func() { _ = root.Close() }()
	// OpenRoot confines operations, including symlink resolution, to UNIC_HOME.
	var f *os.File
	var size int64
	var opened time.Time
	defer func() {
		if f != nil {
			_ = f.Close()
		}
	}()
	write := func(b []byte) error {
		c.mirrorLocal(b)
		if f != nil && (size+int64(len(b)) > 4<<20 || time.Since(opened) >= 7*24*time.Hour) {
			_ = f.Close()
			f = nil
			size = 0
		}
		if err := pruneLogs(root, int64(len(b))); err != nil {
			return err
		}
		if f == nil {
			opened = time.Now()
			var err error
			f, err = root.OpenFile(fmt.Sprintf("segment-%d.jsonl", time.Now().UnixNano()), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
			if err != nil {
				return err
			}
		}
		n, err := f.Write(b)
		size += int64(n)
		return err
	}
	ticker := time.NewTicker(time.Hour)
	defer ticker.Stop()
	for {
		select {
		case b := <-c.local:
			if err := write(b); err != nil {
				c.drop(0, 3, 1)
			}
		case <-ticker.C:
			_ = pruneLogs(root, 0)
		case <-c.localStop:
			for {
				select {
				case b := <-c.local:
					if err := write(b); err != nil {
						c.drop(0, 3, 1)
					}
				default:
					return
				}
			}
		}
	}
}
func (c *Controller) mirrorLocal(b []byte) {
	if c.opts.LocalOutput != nil {
		func() { defer func() { _ = recover() }(); c.opts.LocalOutput(b) }()
	}
}
func (c *Controller) drainLocal(_ error) {
	for {
		select {
		case b := <-c.local:
			c.mirrorLocal(b)
			c.drop(0, 3, 1)
		case <-c.localStop:
			for {
				select {
				case b := <-c.local:
					c.mirrorLocal(b)
					c.drop(0, 3, 1)
				default:
					return
				}
			}
		}
	}
}
func pruneLogs(root *os.Root, incoming int64) error {
	entries, err := fs.ReadDir(root.FS(), ".")
	if err != nil {
		return err
	}
	type segment struct {
		name    string
		created time.Time
		size    int64
	}
	var files []segment
	var total int64
	for _, e := range entries {
		if !strings.HasPrefix(e.Name(), "segment-") || !strings.HasSuffix(e.Name(), ".jsonl") {
			continue
		}
		if e.Type()&os.ModeSymlink != 0 || e.IsDir() {
			return fmt.Errorf("unsafe log entry")
		}
		ns, err := strconv.ParseInt(strings.TrimSuffix(strings.TrimPrefix(e.Name(), "segment-"), ".jsonl"), 10, 64)
		if err != nil {
			continue
		}
		st, err := e.Info()
		if err != nil {
			return err
		}
		files = append(files, segment{e.Name(), time.Unix(0, ns), st.Size()})
		total += st.Size()
	}
	sort.Slice(files, func(i, j int) bool { return files[i].created.Before(files[j].created) })
	for _, f := range files {
		if total+incoming <= 20<<20 && time.Since(f.created) <= 7*24*time.Hour {
			continue
		}
		if err := root.Remove(f.name); err != nil {
			return err
		}
		total -= f.size
	}
	return nil
}
