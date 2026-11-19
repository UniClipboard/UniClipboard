package v1migration

import (
	"context"
	"slices"
	"time"

	"github.com/godbus/dbus/v5"
)

func ReadSecret(service, account string) ([]byte, error) {
	return ReadSecretContext(context.Background(), service, account)
}

// ReadSecretContext bounds both the bus operation and a real collection prompt.
// The Secret Service owns the prompt; this client never asks for its password.
func ReadSecretContext(parent context.Context, service, account string) ([]byte, error) {
	if err := parent.Err(); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(parent, 45*time.Second)
	defer cancel()
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		return nil, credentialError("Linux Secret Service is unavailable; start a session keyring or use the old passphrase")
	}
	defer func() { _ = conn.Close() }()
	object := conn.Object("org.freedesktop.secrets", "/org/freedesktop/secrets")
	var output dbus.Variant
	var session dbus.ObjectPath
	if object.CallWithContext(ctx, "org.freedesktop.Secret.Service.OpenSession", 0, "plain", dbus.MakeVariant("")).Store(&output, &session) != nil {
		return nil, credentialError("Linux Secret Service is unavailable; start a session keyring or use the old passphrase")
	}
	defer conn.Object("org.freedesktop.secrets", session).CallWithContext(ctx, "org.freedesktop.Secret.Session.Close", 0)
	var unlocked, locked []dbus.ObjectPath
	// Match keyring-rs's target-first lookup, then its legacy attribute lookup.
	for _, attrs := range []map[string]string{
		{"service": service, "username": account, "target": "default"},
		{"service": service, "username": account},
	} {
		if object.CallWithContext(ctx, "org.freedesktop.Secret.Service.SearchItems", 0, attrs).Store(&unlocked, &locked) != nil {
			return nil, credentialError("Linux credential lookup failed; unlock the session keyring")
		}
		if len(unlocked)+len(locked) > 0 {
			break
		}
	}
	if len(unlocked)+len(locked) == 0 {
		return nil, credentialError("Linux credential was not found; select the original v1 profile and credential service")
	}
	if len(unlocked)+len(locked) != 1 {
		return nil, credentialError("Linux credential lookup is ambiguous; use the old passphrase")
	}
	if len(locked) > 0 {
		item := locked[0]
		var prompt dbus.ObjectPath
		if object.CallWithContext(ctx, "org.freedesktop.Secret.Service.Unlock", 0, locked).Store(&unlocked, &prompt) != nil {
			return nil, credentialError("Linux keyring unlock failed; unlock the session keyring")
		}
		if prompt != "/" {
			completed, err := completeSecretPrompt(ctx, conn, prompt)
			if err != nil {
				return nil, err
			}
			unlocked = append(unlocked, completed...)
		}
		if !slices.Contains(unlocked, item) {
			return nil, credentialError("Linux keyring remains locked; unlock the session keyring")
		}
		unlocked = []dbus.ObjectPath{item}
	}
	var secret struct {
		Session     dbus.ObjectPath
		Parameters  []byte
		Value       []byte
		ContentType string
	}
	if conn.Object("org.freedesktop.secrets", unlocked[0]).CallWithContext(ctx, "org.freedesktop.Secret.Item.GetSecret", 0, session).Store(&secret) != nil {
		return nil, credentialError("Linux credential access was denied; allow access in the session keyring")
	}
	if len(secret.Value) > 1<<20 {
		clear(secret.Value)
		return nil, ErrFormat
	}
	return secret.Value, nil
}

func completeSecretPrompt(ctx context.Context, conn *dbus.Conn, path dbus.ObjectPath) ([]dbus.ObjectPath, error) {
	options := []dbus.MatchOption{dbus.WithMatchSender("org.freedesktop.secrets"), dbus.WithMatchObjectPath(path), dbus.WithMatchInterface("org.freedesktop.Secret.Prompt"), dbus.WithMatchMember("Completed")}
	if err := conn.AddMatchSignalContext(ctx, options...); err != nil {
		return nil, credentialError("Linux keyring prompt is unavailable; unlock the session keyring")
	}
	defer func() { _ = conn.RemoveMatchSignal(options...) }()
	signals := make(chan *dbus.Signal, 1)
	conn.Signal(signals)
	defer conn.RemoveSignal(signals)
	prompt := conn.Object("org.freedesktop.secrets", path)
	// Subscribe before Prompt: an unlocked keyring can complete synchronously.
	if prompt.CallWithContext(ctx, "org.freedesktop.Secret.Prompt.Prompt", 0, "").Err != nil {
		return nil, credentialError("Linux keyring prompt failed; unlock the session keyring")
	}
	for {
		select {
		case signal, ok := <-signals:
			if !ok {
				return nil, credentialError("Linux keyring connection closed before access was granted")
			}
			if signal.Path != path || signal.Name != "org.freedesktop.Secret.Prompt.Completed" {
				continue
			}
			var dismissed bool
			var result dbus.Variant
			if dbus.Store(signal.Body, &dismissed, &result) != nil {
				return nil, credentialError("Linux keyring returned an invalid prompt result")
			}
			if dismissed {
				return nil, credentialError("Linux keyring prompt was cancelled; retry and allow access")
			}
			var unlocked []dbus.ObjectPath
			if result.Store(&unlocked) != nil {
				return nil, credentialError("Linux keyring returned an invalid unlock result")
			}
			return unlocked, nil
		case <-ctx.Done():
			cleanup, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			_ = prompt.CallWithContext(cleanup, "org.freedesktop.Secret.Prompt.Dismiss", 0).Err
			cancel()
			return nil, credentialError("Linux keyring prompt was cancelled or timed out; retry and allow access")
		}
	}
}
