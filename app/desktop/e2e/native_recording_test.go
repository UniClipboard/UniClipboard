package e2e

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"time"
)

func (r *desktopRecording) startNativeRecording() {
	fail := func(err error) { r.err = err; r.startupErr = err; close(r.done) }
	binary := filepath.Join(r.s.private, "window-recorder")
	ctx, cancel := context.WithTimeout(r.s.ctx, 90*time.Second)
	defer cancel()
	build := exec.CommandContext(ctx, "swiftc", filepath.Join(r.s.root, "app/desktop/e2e/window_recording.swift"), "-o", binary)
	if out, err := build.CombinedOutput(); err != nil {
		fail(fmt.Errorf("build window recorder: %w: %s", err, out))
		return
	}
	target := filepath.Join(r.s.out, "recording.mp4")
	command := exec.Command(binary, strconv.Itoa(r.s.app.Process.Pid), target)
	input, err := command.StdinPipe()
	if err != nil {
		fail(err)
		return
	}
	output, err := command.StdoutPipe()
	if err != nil {
		_ = input.Close()
		fail(err)
		return
	}
	log, err := os.Create(filepath.Join(r.s.private, "window-recorder.log"))
	if err != nil {
		_ = input.Close()
		fail(err)
		return
	}
	command.Stderr = log
	if err = command.Start(); err != nil {
		_ = input.Close()
		_ = log.Close()
		fail(err)
		return
	}
	r.nativeCmd, r.nativeInput, r.nativeDone = command, input, make(chan error, 1)
	ready := make(chan struct{})
	go func() {
		defer func() { _ = log.Close() }()
		scanner := bufio.NewScanner(output)
		scanner.Buffer(make([]byte, 65536), 8<<20)
		signaled := false
		for scanner.Scan() {
			raw := append([]byte(nil), scanner.Bytes()...)
			var message struct {
				Ready  bool              `json:"ready"`
				Error  string            `json:"error"`
				Frames []json.RawMessage `json:"frames"`
			}
			if err := json.Unmarshal(raw, &message); err != nil {
				r.nativeDone <- err
				_ = command.Process.Kill()
				_ = command.Wait()
				return
			}
			if message.Ready && !signaled {
				close(ready)
				signaled = true
			}
			if message.Error != "" {
				r.err = errors.New(message.Error)
			}
			if len(message.Frames) > 0 {
				r.nativeReceipt = raw
			}
		}
		scanErr := scanner.Err()
		waitErr := command.Wait()
		if waitErr != nil {
			if data, err := os.ReadFile(filepath.Join(r.s.private, "window-recorder.log")); err == nil && len(data) > 0 {
				waitErr = fmt.Errorf("%w: %s", waitErr, data[:min(len(data), 1500)])
			}
		}
		r.nativeDone <- errors.Join(scanErr, waitErr)
	}()
	select {
	case <-ready:
		r.started = time.Now()
	case err := <-r.nativeDone:
		r.nativeCmd = nil
		fail(errors.Join(errors.New("native window recorder did not start"), err, r.err))
	case <-time.After(20 * time.Second):
		_ = command.Process.Kill()
		<-r.nativeDone
		r.nativeCmd = nil
		fail(errors.New("native window recorder did not produce its first frame"))
	}
}

func (r *desktopRecording) stopNativeRecording() error {
	_, _ = r.nativeInput.Write([]byte("stop\n"))
	_ = r.nativeInput.Close()
	select {
	case err := <-r.nativeDone:
		if err != nil || r.err != nil {
			return errors.Join(err, r.err)
		}
	case <-time.After(20 * time.Second):
		_ = r.nativeCmd.Process.Kill()
		<-r.nativeDone
		return errors.New("native window recorder did not finalize")
	}
	var receipt struct {
		Frames []struct {
			At float64 `json:"at_seconds"`
		} `json:"frames"`
		Duration float64 `json:"duration_seconds"`
	}
	if err := json.Unmarshal(r.nativeReceipt, &receipt); err != nil {
		return err
	}
	if len(receipt.Frames) < 2 || receipt.Duration <= 0 {
		return errors.New("empty native window recording")
	}
	target := filepath.Join(r.s.out, "recording.mp4")
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if out, err := exec.CommandContext(ctx, "ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", target).CombinedOutput(); err != nil {
		return fmt.Errorf("invalid native video: %w: %s", err, out)
	}
	if err := os.WriteFile(filepath.Join(r.s.out, "recording-frames.json"), r.nativeReceipt, 0600); err != nil {
		return err
	}
	r.s.result.RecordingStarted = r.started.Sub(r.s.started).Seconds()
	r.s.result.Recording = "recording.mp4"
	r.s.result.Frames = len(receipt.Frames)
	r.s.result.Duration = receipt.Duration
	return nil
}
