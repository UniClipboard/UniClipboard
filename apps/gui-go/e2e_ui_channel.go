//go:build e2e

package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"sync"
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
		uiChild = true
		var traceMu sync.Mutex
		var trace []int
		var peer *uiPeer
		peer = newUIPeer(input, output, func(m uiPacket) (any, error) {
			switch m.Operation {
			case "notification":
				var number int
				if err := json.Unmarshal(m.Data, &number); err != nil {
					return nil, err
				}
				if number == 0 {
					ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
					defer cancel()
					if err := peer.call(ctx, "notification-gate", nil, nil); err != nil {
						return nil, err
					}
				}
				traceMu.Lock()
				trace = append(trace, number)
				traceMu.Unlock()
				return nil, nil
			case "notification-trace":
				traceMu.Lock()
				result := append([]int(nil), trace...)
				traceMu.Unlock()
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				return nil, peer.call(ctx, "notification-result", result, nil)
			case "roundtrip":
				var result string
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				err := peer.call(ctx, "reverse", "child-to-parent", &result)
				return result, err
			case "forward-theme-error":
				return (&HostService{owner: peer}).SetFollowOmarchyTheme(true)
			case "forward-restart-error":
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				return nil, (&HostService{owner: peer}).RestartDaemon(ctx)
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
		fmt.Println(`{"passed":true,"scope":"real inherited-pipe transport; no Wails app or daemon","checks":["bidirectional request","structured error","owner command error forwarding","text error","timeout isolation","ordered notifications","request while notification awaits reverse reply","parent EOF and child reap"]}`)
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
	gateStarted := make(chan struct{})
	gateRelease := make(chan struct{})
	traceResult := make(chan []int, 1)
	defer func() {
		select {
		case <-gateRelease:
		default:
			close(gateRelease)
		}
	}()
	peer := newUIPeer(receive, send, func(m uiPacket) (any, error) {
		if m.Operation == "ready" {
			ready <- struct{}{}
			return nil, nil
		}
		if m.Operation == "desktop-theme-follow" || m.Operation == "restart-daemon" {
			return nil, hostapi.New(hostapi.CodeConflict, "owner probe refusal")
		}
		if m.Operation == "notification-gate" {
			close(gateStarted)
			<-gateRelease
			return nil, nil
		}
		if m.Operation == "notification-result" {
			var result []int
			if err := json.Unmarshal(m.Data, &result); err != nil {
				return nil, err
			}
			traceResult <- result
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
	for _, operation := range []string{"forward-theme-error", "forward-restart-error"} {
		err := peer.call(ctx, operation, nil, nil)
		if !errors.As(err, &command) || command.Code != hostapi.CodeConflict || command.Message != "owner probe refusal" {
			return fmt.Errorf("owner error forwarding %s: %v", operation, err)
		}
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
	const notificationCount = 65
	if err := peer.send(uiPacket{Operation: "notification", Data: json.RawMessage("0")}); err != nil {
		return err
	}
	select {
	case <-gateStarted:
	case <-ctx.Done():
		return errors.New("first notification did not start")
	}
	for i := 1; i < notificationCount; i++ {
		data, _ := json.Marshal(i)
		if err := peer.send(uiPacket{Operation: "notification", Data: data}); err != nil {
			return err
		}
	}
	if err := peer.send(uiPacket{Operation: "notification-trace"}); err != nil {
		return err
	}
	if err := peer.call(ctx, "echo", "reader-not-blocked", &value); err != nil || value != "reader-not-blocked" {
		return fmt.Errorf("request during notification: %q %v", value, err)
	}
	close(gateRelease)
	select {
	case trace := <-traceResult:
		if len(trace) != notificationCount {
			return fmt.Errorf("notification count: got %d, want %d", len(trace), notificationCount)
		}
		for i, value := range trace {
			if value != i {
				return fmt.Errorf("notification order at %d: got %d", i, value)
			}
		}
	case <-ctx.Done():
		return errors.New("notification trace timed out")
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
