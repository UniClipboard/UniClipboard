package e2e

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

type capturedFrame struct {
	File string  `json:"file"`
	At   float64 `json:"at_seconds"`
}
type desktopRecording struct {
	s           *desktopScenario
	stopRequest chan struct{}
	done        chan struct{}
	frames      []capturedFrame
	err         error
	started     time.Time
}

func newDesktopRecording(s *desktopScenario) *desktopRecording {
	r := &desktopRecording{s: s, stopRequest: make(chan struct{}), done: make(chan struct{}), started: time.Now()}
	go r.run()
	return r
}
func (r *desktopRecording) run() {
	defer close(r.done)
	dir := filepath.Join(r.s.private, "frames")
	if r.err = os.MkdirAll(dir, 0700); r.err != nil {
		return
	}
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	for {
		data, err := r.s.capture()
		if err != nil {
			r.err = err
			return
		}
		name := fmt.Sprintf("%06d.png", len(r.frames))
		if r.err = os.WriteFile(filepath.Join(dir, name), data, 0600); r.err != nil {
			return
		}
		r.frames = append(r.frames, capturedFrame{File: name, At: time.Since(r.started).Seconds()})
		select {
		case <-r.stopRequest:
			return
		case <-r.s.ctx.Done():
			r.err = r.s.ctx.Err()
			return
		case <-ticker.C:
		}
	}
}
func (r *desktopRecording) stop() error {
	close(r.stopRequest)
	select {
	case <-r.done:
	case <-time.After(40 * time.Second):
		return errors.New("recording did not stop")
	}
	if r.err != nil {
		return fmt.Errorf("window recording: %w", r.err)
	}
	if len(r.frames) < 2 {
		return errors.New("native case produced fewer than two recording frames")
	}
	var list strings.Builder
	for i, frame := range r.frames {
		duration := 0.25
		if i+1 < len(r.frames) {
			duration = r.frames[i+1].At - frame.At
		}
		fmt.Fprintf(&list, "file '%s'\nduration %.6f\n", frame.File, duration)
	}
	fmt.Fprintf(&list, "file '%s'\n", r.frames[len(r.frames)-1].File)
	dir := filepath.Join(r.s.private, "frames")
	if err := os.WriteFile(filepath.Join(dir, "frames.ffconcat"), []byte(list.String()), 0600); err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	target := filepath.Join(r.s.out, "recording.mp4")
	cmd := exec.CommandContext(ctx, "ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", "frames.ffconcat", "-an", "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-fps_mode", "vfr", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", target)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("recording encoder: %v (%s)", err, string(out))
	}
	output, err := exec.CommandContext(ctx, "ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", target).Output()
	if err != nil {
		return errors.New("recording is not playable")
	}
	var probe struct{ Format struct{ Duration string } }
	if json.Unmarshal(output, &probe) != nil {
		return errors.New("invalid recording metadata")
	}
	var duration float64
	if _, err = fmt.Sscan(probe.Format.Duration, &duration); err != nil || duration <= 0 {
		return errors.New("empty recording")
	}
	r.s.result.RecordingStarted = r.started.Sub(r.s.started).Seconds() + r.frames[0].At
	r.s.result.Recording = "recording.mp4"
	r.s.result.Frames = len(r.frames)
	r.s.result.Duration = duration
	metadata, err := json.MarshalIndent(r.frames, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(r.s.out, "recording-frames.json"), metadata, 0600)
}
