//go:build linux

package quickpanel

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/godbus/dbus/v5"
)

const (
	portalBusName       = "org.freedesktop.portal.Desktop"
	portalObjectPath    = dbus.ObjectPath("/org/freedesktop/portal/desktop")
	portalRemoteDesktop = "org.freedesktop.portal.RemoteDesktop"
	portalRequestIface  = "org.freedesktop.portal.Request"
	portalSessionIface  = "org.freedesktop.portal.Session"

	portalDeviceKeyboard uint32 = 1
	portalPersistAlways  uint32 = 2
	waylandTokenFileName        = "wayland-paste-token"
)

type waylandPortalBackend struct {
	mu         sync.Mutex
	profileDir string
	conn       *dbus.Conn
	session    dbus.ObjectPath
	closedCh   chan *dbus.Signal
}

func connectPrivateSessionBus() (*dbus.Conn, error) {
	conn, err := dbus.SessionBusPrivate()
	if err != nil {
		return nil, err
	}
	if err := conn.Auth(nil); err != nil {
		_ = conn.Close()
		return nil, err
	}
	if err := conn.Hello(); err != nil {
		_ = conn.Close()
		return nil, err
	}
	return conn, nil
}

func probeWaylandRemoteDesktopPortal() bool {
	conn, err := connectPrivateSessionBus()
	if err != nil {
		return false
	}
	defer func() { _ = conn.Close() }()

	obj := conn.Object(portalBusName, portalObjectPath)
	verVar, err := obj.GetProperty(portalRemoteDesktop + ".version")
	if err != nil {
		return false
	}
	ver, ok := verVar.Value().(uint32)
	if !ok || ver < 1 {
		return false
	}
	devVar, err := obj.GetProperty(portalRemoteDesktop + ".AvailableDeviceTypes")
	if err != nil {
		return false
	}
	devices, ok := devVar.Value().(uint32)
	return ok && (devices&portalDeviceKeyboard) != 0
}

func (b *waylandPortalBackend) SetProfileDir(dir string) {
	b.mu.Lock()
	b.profileDir = dir
	b.mu.Unlock()
}

func (b *waylandPortalBackend) tokenPathLocked() string {
	dir := strings.TrimSpace(b.profileDir)
	if dir == "" {
		dir = strings.TrimSpace(os.Getenv("UNIC_HOME"))
	}
	if dir == "" {
		return ""
	}
	return filepath.Join(dir, waylandTokenFileName)
}

func (b *waylandPortalBackend) loadTokenLocked() string {
	p := b.tokenPathLocked()
	if p == "" {
		return ""
	}
	raw, err := os.ReadFile(p)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(raw))
}

func (b *waylandPortalBackend) saveTokenLocked(token string) {
	p := b.tokenPathLocked()
	token = strings.TrimSpace(token)
	if p == "" || token == "" {
		return
	}
	tmp := p + ".tmp"
	if err := os.WriteFile(tmp, []byte(token+"\n"), 0600); err != nil {
		return
	}
	if err := os.Rename(tmp, p); err != nil {
		_ = os.Remove(tmp)
	}
}

func (b *waylandPortalBackend) Prepare(ctx context.Context) error {
	b.mu.Lock()
	b.drainClosedLocked()
	if b.conn != nil && b.session != "" {
		b.mu.Unlock()
		return nil
	}
	savedToken := b.loadTokenLocked()
	b.mu.Unlock()

	conn, err := connectPrivateSessionBus()
	if err != nil {
		return ErrInput
	}
	cleanupConn := true
	defer func() {
		if cleanupConn {
			_ = conn.Close()
		}
	}()

	// Allow ample time for first-time native GNOME/KDE consent dialog while
	// remaining cancellable by context.
	pctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()

	// 1. CreateSession
	createTok := randomPortalToken()
	sessTok := randomPortalToken()
	code, results, err := callPortalRequest(pctx, conn, "CreateSession", func(obj dbus.BusObject) *dbus.Call {
		return obj.CallWithContext(pctx, portalRemoteDesktop+".CreateSession", 0, map[string]dbus.Variant{
			"handle_token":         dbus.MakeVariant(createTok),
			"session_handle_token": dbus.MakeVariant(sessTok),
		})
	}, createTok)
	if err != nil {
		return err
	}
	if code != 0 {
		return ErrPermission
	}
	sessionStr, _ := results["session_handle"].Value().(string)
	if sessionStr == "" {
		return ErrInput
	}
	sessionPath := dbus.ObjectPath(sessionStr)
	closeSession := func() {
		_ = conn.Object(portalBusName, sessionPath).Call(portalSessionIface+".Close", 0).Err
	}

	// 2. SelectDevices (KEYBOARD, persist_mode=2, optional restore_token)
	selectTok := randomPortalToken()
	selectOpts := map[string]dbus.Variant{
		"handle_token": dbus.MakeVariant(selectTok),
		"types":        dbus.MakeVariant(portalDeviceKeyboard),
		"persist_mode": dbus.MakeVariant(portalPersistAlways),
	}
	if savedToken != "" {
		selectOpts["restore_token"] = dbus.MakeVariant(savedToken)
	}
	code, _, err = callPortalRequest(pctx, conn, "SelectDevices", func(obj dbus.BusObject) *dbus.Call {
		return obj.CallWithContext(pctx, portalRemoteDesktop+".SelectDevices", 0, sessionPath, selectOpts)
	}, selectTok)
	if err != nil {
		closeSession()
		return err
	}
	if code != 0 {
		closeSession()
		return ErrPermission
	}

	// 3. Start (may show native GNOME/KDE consent dialog on first launch; silent with valid restore_token)
	startTok := randomPortalToken()
	code, startResults, err := callPortalRequest(pctx, conn, "Start", func(obj dbus.BusObject) *dbus.Call {
		return obj.CallWithContext(pctx, portalRemoteDesktop+".Start", 0, sessionPath, "", map[string]dbus.Variant{
			"handle_token": dbus.MakeVariant(startTok),
		})
	}, startTok)
	if err != nil {
		closeSession()
		return err
	}
	if code != 0 {
		closeSession()
		return ErrPermission
	}
	devices, _ := startResults["devices"].Value().(uint32)
	if (devices & portalDeviceKeyboard) == 0 {
		closeSession()
		return ErrPermission
	}

	closedCh := make(chan *dbus.Signal, 4)
	if err := conn.AddMatchSignal(
		dbus.WithMatchSender(portalBusName),
		dbus.WithMatchObjectPath(sessionPath),
		dbus.WithMatchInterface(portalSessionIface),
		dbus.WithMatchMember("Closed"),
	); err == nil {
		conn.Signal(closedCh)
	}

	b.mu.Lock()
	b.closeLocked()
	b.conn = conn
	b.session = sessionPath
	b.closedCh = closedCh
	if newToken, ok := startResults["restore_token"].Value().(string); ok && newToken != "" {
		b.saveTokenLocked(newToken)
	}
	b.mu.Unlock()
	cleanupConn = false
	return nil
}

func (b *waylandPortalBackend) PasteCtrlV() error {
	b.mu.Lock()
	b.drainClosedLocked()
	conn := b.conn
	session := b.session
	b.mu.Unlock()

	if conn == nil || session == "" {
		return ErrPermission
	}
	obj := conn.Object(portalBusName, portalObjectPath)
	opts := map[string]dbus.Variant{}
	sendKey := func(keycode int32, state uint32) error {
		return obj.Call(portalRemoteDesktop+".NotifyKeyboardKeycode", 0, session, opts, keycode, state).Err
	}
	if err := sendKey(int32(linuxKeyLeftCtrl), 1); err != nil {
		b.resetSession()
		return ErrInput
	}
	if err := sendKey(int32(linuxKeyV), 1); err != nil {
		_ = sendKey(int32(linuxKeyLeftCtrl), 0)
		b.resetSession()
		return ErrInput
	}
	time.Sleep(15 * time.Millisecond)
	errV := sendKey(int32(linuxKeyV), 0)
	errCtrl := sendKey(int32(linuxKeyLeftCtrl), 0)
	if errV != nil || errCtrl != nil {
		b.resetSession()
		return ErrInput
	}
	return nil
}

func (b *waylandPortalBackend) Close() {
	b.mu.Lock()
	b.closeLocked()
	b.mu.Unlock()
}

func (b *waylandPortalBackend) resetSession() {
	b.mu.Lock()
	b.closeLocked()
	b.mu.Unlock()
}

func (b *waylandPortalBackend) drainClosedLocked() {
	if b.closedCh == nil {
		return
	}
	for {
		select {
		case sig := <-b.closedCh:
			if sig != nil && sig.Path == b.session {
				b.closeLocked()
				return
			}
		default:
			return
		}
	}
}

func (b *waylandPortalBackend) closeLocked() {
	if b.conn != nil {
		if b.closedCh != nil {
			b.conn.RemoveSignal(b.closedCh)
			b.closedCh = nil
		}
		if b.session != "" {
			_ = b.conn.Object(portalBusName, b.session).Call(portalSessionIface+".Close", 0).Err
			b.session = ""
		}
		_ = b.conn.Close()
		b.conn = nil
	}
}

func callPortalRequest(
	ctx context.Context,
	conn *dbus.Conn,
	_ string,
	invoke func(dbus.BusObject) *dbus.Call,
	handleToken string,
) (uint32, map[string]dbus.Variant, error) {
	sender := strings.TrimPrefix(conn.Names()[0], ":")
	sender = strings.ReplaceAll(sender, ".", "_")
	expectedPath := dbus.ObjectPath("/org/freedesktop/portal/desktop/request/" + sender + "/" + handleToken)

	sigCh := make(chan *dbus.Signal, 4)
	matchOpts := []dbus.MatchOption{
		dbus.WithMatchSender(portalBusName),
		dbus.WithMatchInterface(portalRequestIface),
		dbus.WithMatchMember("Response"),
	}
	if err := conn.AddMatchSignal(matchOpts...); err != nil {
		return 0, nil, ErrInput
	}
	defer func() { _ = conn.RemoveMatchSignal(matchOpts...) }()
	conn.Signal(sigCh)
	defer conn.RemoveSignal(sigCh)

	call := invoke(conn.Object(portalBusName, portalObjectPath))
	if call.Err != nil {
		return 0, nil, ErrInput
	}
	var actualPath dbus.ObjectPath
	if err := call.Store(&actualPath); err != nil {
		return 0, nil, ErrInput
	}
	if actualPath == "" {
		actualPath = expectedPath
	}

	for {
		select {
		case <-ctx.Done():
			_ = conn.Object(portalBusName, actualPath).Call(portalRequestIface+".Close", 0).Err
			return 0, nil, ctx.Err()
		case sig := <-sigCh:
			if sig == nil || sig.Path != actualPath || len(sig.Body) < 2 {
				continue
			}
			code, ok1 := sig.Body[0].(uint32)
			res, ok2 := sig.Body[1].(map[string]dbus.Variant)
			if !ok1 || !ok2 {
				return 0, nil, errors.New("portal: malformed response")
			}
			return code, res, nil
		}
	}
}

func randomPortalToken() string {
	var raw [8]byte
	_, _ = rand.Read(raw[:])
	return "unic_" + hex.EncodeToString(raw[:])
}
