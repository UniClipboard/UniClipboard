//go:build linux

package quickpanel

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"net"
	"os"
	"path/filepath"
	"sync/atomic"
	"syscall"
	"testing"
	"time"
)

type fakeWaylandBackend struct {
	prepareErr error
	pasteErr   error
	prepares   atomic.Int32
	pastes     atomic.Int32
	closed     atomic.Bool
}

func (f *fakeWaylandBackend) Prepare(context.Context) error {
	f.prepares.Add(1)
	return f.prepareErr
}
func (f *fakeWaylandBackend) PasteCtrlV() error {
	f.pastes.Add(1)
	return f.pasteErr
}
func (f *fakeWaylandBackend) Close() { f.closed.Store(true) }

func TestWaylandPlatformLifecycle(t *testing.T) {
	backend := &fakeWaylandBackend{}
	p := newWaylandPlatformWithBackend(backend)
	p.settleDelay = 25 * time.Millisecond

	var focused atomic.Bool
	focused.Store(true)
	p.SetFocusChecker(focused.Load)

	target, err := p.Capture()
	if err != nil {
		t.Fatalf("Capture: %v", err)
	}
	if target.ID != waylandCompositorTargetID || target.PID != 0 {
		t.Fatalf("unexpected Wayland target: %+v", target)
	}
	if !p.OwnsForeground() {
		t.Fatal("expected OwnsForeground while panel window is focused")
	}
	if err := p.Check(Target{ID: 1234, PID: 99}); !errors.Is(err, ErrTarget) {
		t.Fatalf("expected ErrTarget for non-Wayland target, got %v", err)
	}

	if err := p.Activate(target); err != nil {
		t.Fatalf("Activate: %v", err)
	}
	// Still focused -> not ready.
	if ready, err := p.Ready(target); err != nil || ready {
		t.Fatalf("expected not ready while panel focused: ready=%v err=%v", ready, err)
	}
	focused.Store(false)
	// Just unfocused, before settleDelay -> not ready yet.
	if ready, err := p.Ready(target); err != nil || ready {
		t.Fatalf("expected settle delay before ready: ready=%v err=%v", ready, err)
	}
	time.Sleep(35 * time.Millisecond)
	if ready, err := p.Ready(target); err != nil || !ready {
		t.Fatalf("expected ready after focus leave + settle: ready=%v err=%v", ready, err)
	}
	if err := p.PreparePaste(context.Background()); err != nil {
		t.Fatalf("PreparePaste: %v", err)
	}
	if err := p.Paste(target); err != nil {
		t.Fatalf("Paste: %v", err)
	}
	if backend.prepares.Load() != 1 || backend.pastes.Load() != 1 {
		t.Fatalf("expected 1 prepare and 1 paste, got %d / %d", backend.prepares.Load(), backend.pastes.Load())
	}
	p.Close()
	if !backend.closed.Load() {
		t.Fatal("Close did not close backend")
	}
}

func TestWaylandPortalTokenPersistence(t *testing.T) {
	dir := t.TempDir()
	b := &waylandPortalBackend{}
	b.SetProfileDir(dir)

	b.mu.Lock()
	if got := b.loadTokenLocked(); got != "" {
		b.mu.Unlock()
		t.Fatalf("expected empty initial token, got %q", got)
	}
	b.saveTokenLocked("opaque-restore-token-123")
	got := b.loadTokenLocked()
	b.mu.Unlock()
	if got != "opaque-restore-token-123" {
		t.Fatalf("token mismatch: got %q", got)
	}
	info, err := os.Stat(filepath.Join(dir, waylandTokenFileName))
	if err != nil {
		t.Fatalf("stat token file: %v", err)
	}
	if info.Mode().Perm() != 0600 {
		t.Fatalf("expected 0600 permissions, got %o", info.Mode().Perm())
	}
}

func TestWaylandVirtualKeyboardWireProtocol(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "wayland-test")
	ln, err := net.ListenUnix("unix", &net.UnixAddr{Name: sockPath, Net: "unix"})
	if err != nil {
		t.Fatalf("listen unix: %v", err)
	}
	defer func() { _ = ln.Close() }()

	type keyEvent struct {
		key   uint32
		state uint32
	}
	var (
		gotKeymap []byte
		gotKeys   []keyEvent
		gotMods   []uint32
		destroyed bool
		srvDone   = make(chan error, 1)
	)

	go func() {
		// Handle 2 connections: 1) probeWaylandVirtualKeyboard, 2) PasteCtrlV.
		for connIdx := 0; connIdx < 2; connIdx++ {
			uc, err := ln.AcceptUnix()
			if err != nil {
				srvDone <- err
				return
			}
			_ = uc.SetDeadline(time.Now().Add(3 * time.Second))
			var (
				regID uint32
				vkID  uint32
				buf   []byte
				tmp   = make([]byte, 4096)
				oob   = make([]byte, 512)
			)
			for {
				n, oobn, _, _, err := uc.ReadMsgUnix(tmp, oob)
				if err != nil {
					break
				}
				if oobn > 0 {
					scms, err := syscall.ParseSocketControlMessage(oob[:oobn])
					if err == nil {
						for _, scm := range scms {
							fds, err := syscall.ParseUnixRights(&scm)
							if err == nil {
								for _, fd := range fds {
									f := os.NewFile(uintptr(fd), "keymap")
									data, _ := os.ReadFile(f.Name())
									if len(data) == 0 {
										var b bytes.Buffer
										_, _ = b.ReadFrom(f)
										data = b.Bytes()
									}
									gotKeymap = data
									_ = f.Close()
								}
							}
						}
					}
				}
				buf = append(buf, tmp[:n]...)
				for len(buf) >= 8 {
					oid := binary.LittleEndian.Uint32(buf[0:4])
					szOp := binary.LittleEndian.Uint32(buf[4:8])
					size := int(szOp >> 16)
					op := uint16(szOp & 0xffff)
					if len(buf) < size {
						break
					}
					payload := buf[8:size]
					buf = buf[size:]
					switch {
					case oid == wlDisplayID && op == wlOpGetReg:
						regID = binary.LittleEndian.Uint32(payload[0:4])
						// Advertise wl_seat and zwp_virtual_keyboard_manager_v1.
						sendGlobal := func(name uint32, iface string, ver uint32) {
							s := encodeWaylandString(iface)
							p := make([]byte, 4+len(s)+4)
							binary.LittleEndian.PutUint32(p[0:4], name)
							copy(p[4:], s)
							binary.LittleEndian.PutUint32(p[4+len(s):], ver)
							msg := make([]byte, 8+len(p))
							binary.LittleEndian.PutUint32(msg[0:4], regID)
							binary.LittleEndian.PutUint32(msg[4:8], (uint32(len(msg))<<16)|uint32(wlRegistryEvGlob))
							copy(msg[8:], p)
							_, _ = uc.Write(msg)
						}
						sendGlobal(10, "wl_seat", 9)
						sendGlobal(20, "zwp_virtual_keyboard_manager_v1", 1)
					case oid == wlDisplayID && op == wlOpSync:
						cbID := binary.LittleEndian.Uint32(payload[0:4])
						msg := make([]byte, 12)
						binary.LittleEndian.PutUint32(msg[0:4], cbID)
						binary.LittleEndian.PutUint32(msg[4:8], (12<<16)|uint32(wlCallbackEvDone))
						binary.LittleEndian.PutUint32(msg[8:12], 0)
						_, _ = uc.Write(msg)
					case oid == 5 && op == vkMgrOpCreate:
						vkID = binary.LittleEndian.Uint32(payload[4:8])
					case vkID != 0 && oid == vkID && op == vkOpKey:
						gotKeys = append(gotKeys, keyEvent{
							key:   binary.LittleEndian.Uint32(payload[4:8]),
							state: binary.LittleEndian.Uint32(payload[8:12]),
						})
					case vkID != 0 && oid == vkID && op == vkOpModifiers:
						gotMods = append(gotMods, binary.LittleEndian.Uint32(payload[0:4]))
					case vkID != 0 && oid == vkID && op == vkOpDestroy:
						destroyed = true
					}
				}
			}
			_ = uc.Close()
		}
		srvDone <- nil
	}()

	if !probeWaylandVirtualKeyboard(sockPath) {
		t.Fatal("probeWaylandVirtualKeyboard returned false on mock server")
	}
	backend := &waylandVirtualKeyboardBackend{socketPath: sockPath}
	if err := backend.PasteCtrlV(); err != nil {
		t.Fatalf("PasteCtrlV: %v", err)
	}
	if err := <-srvDone; err != nil {
		t.Fatalf("mock server error: %v", err)
	}

	if len(gotKeymap) == 0 || !bytes.Contains(gotKeymap, []byte("xkb_keymap")) || gotKeymap[len(gotKeymap)-1] != 0 {
		t.Fatalf("keymap invalid: got %d bytes", len(gotKeymap))
	}
	wantKeys := []keyEvent{
		{linuxKeyLeftCtrl, 1},
		{linuxKeyV, 1},
		{linuxKeyV, 0},
		{linuxKeyLeftCtrl, 0},
	}
	if len(gotKeys) != len(wantKeys) {
		t.Fatalf("key sequence mismatch: got %+v, want %+v", gotKeys, wantKeys)
	}
	for i := range wantKeys {
		if gotKeys[i] != wantKeys[i] {
			t.Fatalf("key[%d]=%+v, want %+v", i, gotKeys[i], wantKeys[i])
		}
	}
	if len(gotMods) != 2 || gotMods[0] != vkModControl || gotMods[1] != 0 {
		t.Fatalf("modifiers sequence mismatch: got %+v", gotMods)
	}
	if !destroyed {
		t.Fatal("virtual keyboard was not destroyed")
	}
}

func TestWaylandVirtualKeyboardLive(t *testing.T) {
	if os.Getenv("UNIC_TEST_LIVE_WAYLAND_VK") != "1" {
		t.Skip("set UNIC_TEST_LIVE_WAYLAND_VK=1 to run against a live Wayland compositor")
	}
	sock := waylandSocketPath()
	t.Logf("socket=%q keymapLen=%d", sock, len(defaultWaylandXKBKeymap()))
	if !probeWaylandVirtualKeyboard(sock) {
		t.Fatalf("probeWaylandVirtualKeyboard(%q) = false", sock)
	}
	b := &waylandVirtualKeyboardBackend{socketPath: sock}
	if err := b.PasteCtrlV(); err != nil {
		t.Fatalf("PasteCtrlV failed: %v", err)
	}
}
