//go:build e2e

package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
)

// This CLI-only probe exits before main, App.New, profile access or daemon bootstrap.
func init() {
	if len(os.Args) < 2 {
		return
	}
	switch os.Args[1] {
	case "--ui-channel-probe-child":
		input, output, err := inheritedUIPipes()
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		var peer *uiPeer
		peer = newUIPeer(input, output, func(m uiPacket) (any, error) {
			switch m.Operation {
			case "roundtrip":
				var result string
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				err := peer.call(ctx, "reverse", "child-to-parent", &result)
				return result, err
			case "command-error":
				return nil, hostapi.New(hostapi.CodeConflict, "probe refusal")
			case "text-error":
				return nil, hostapi.TextError("probe text")
			case "slow":
				time.Sleep(100 * time.Millisecond)
				return "late", nil
			case "echo":
				var value string
				err := json.Unmarshal(m.Data, &value)
				return value, err
			default:
				return nil, errors.New("unknown probe request")
			}
		})
		_ = peer.send(uiPacket{Operation: "ready"})
		<-peer.done
		os.Exit(0)
	case "--ui-channel-probe":
		if err := runUIChannelProbe(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Println(`{"passed":true,"scope":"real inherited-pipe transport; no Wails app or daemon","checks":["bidirectional request","structured error","text error","timeout isolation","parent EOF and child reap"]}`)
		os.Exit(0)
	}
}

func runUIChannelProbe() error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	input, send, err := os.Pipe()
	if err != nil {
		return err
	}
	receive, output, err := os.Pipe()
	if err != nil {
		return err
	}
	cmd := exec.Command(exe, "--ui-channel-probe-child")
	cmd.Stderr = os.Stderr
	if err := startUICommand(cmd, input, output); err != nil {
		return err
	}
	input.Close()
	output.Close()
	ready := make(chan struct{}, 1)
	peer := newUIPeer(receive, send, func(m uiPacket) (any, error) {
		if m.Operation == "ready" {
			ready <- struct{}{}
			return nil, nil
		}
		if m.Operation == "reverse" {
			var value string
			err := json.Unmarshal(m.Data, &value)
			return value, err
		}
		return nil, errors.New("unknown reverse request")
	})
	defer peer.close()
	reaped := false
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	defer func() {
		if reaped {
			return
		}
		peer.close()
		select {
		case <-done:
		case <-time.After(2 * time.Second):
			_ = cmd.Process.Kill()
		}
	}()
	select {
	case <-ready:
	case <-time.After(5 * time.Second):
		return errors.New("child readiness timeout")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var value string
	if err := peer.call(ctx, "roundtrip", nil, &value); err != nil || value != "child-to-parent" {
		return fmt.Errorf("reverse request: %q %v", value, err)
	}
	var command hostapi.CommandError
	if err := peer.call(ctx, "command-error", nil, nil); !errors.As(err, &command) || command.Code != hostapi.CodeConflict {
		return fmt.Errorf("structured error: %v", err)
	}
	var text hostapi.TextError
	if err := peer.call(ctx, "text-error", nil, nil); !errors.As(err, &text) || text != "probe text" {
		return fmt.Errorf("text error: %v", err)
	}
	short, stop := context.WithTimeout(ctx, 10*time.Millisecond)
	err = peer.call(short, "slow", nil, nil)
	stop()
	if !errors.Is(err, context.DeadlineExceeded) {
		return fmt.Errorf("request deadline: %v", err)
	}
	if err := peer.call(ctx, "echo", "still-alive", &value); err != nil || value != "still-alive" {
		return fmt.Errorf("request after timeout: %q %v", value, err)
	}
	peer.close()
	select {
	case err := <-done:
		reaped = true
		return err
	case <-time.After(5 * time.Second):
		return errors.New("child survived parent EOF")
	}
}
