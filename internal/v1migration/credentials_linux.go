package v1migration

import (
	"context"
	"time"

	"github.com/godbus/dbus/v5"
)

func ReadSecret(service, account string) ([]byte, error) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		return nil, ErrUnlock
	}
	defer func() { _ = conn.Close() }()
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	object := conn.Object("org.freedesktop.secrets", "/org/freedesktop/secrets")
	var output dbus.Variant
	var session dbus.ObjectPath
	if object.CallWithContext(ctx, "org.freedesktop.Secret.Service.OpenSession", 0, "plain", dbus.MakeVariant("")).Store(&output, &session) != nil {
		return nil, ErrUnlock
	}
	defer conn.Object("org.freedesktop.secrets", session).Call("org.freedesktop.Secret.Session.Close", 0)
	var unlocked, locked []dbus.ObjectPath
	// Match keyring-rs's target-first lookup, then its legacy attribute lookup.
	for _, attrs := range []map[string]string{
		{"service": service, "username": account, "target": "default"},
		{"service": service, "username": account},
	} {
		if object.CallWithContext(ctx, "org.freedesktop.Secret.Service.SearchItems", 0, attrs).Store(&unlocked, &locked) != nil {
			return nil, ErrUnlock
		}
		if len(unlocked)+len(locked) > 0 {
			break
		}
	}
	if len(unlocked)+len(locked) != 1 {
		return nil, ErrUnlock
	}
	if len(locked) > 0 {
		var prompt dbus.ObjectPath
		if object.CallWithContext(ctx, "org.freedesktop.Secret.Service.Unlock", 0, locked).Store(&unlocked, &prompt) != nil {
			return nil, ErrUnlock
		}
		// An interactive collection prompt belongs to the host. Fall back to
		// the old passphrase rather than silently accessing another store.
		if prompt != "/" || len(unlocked) != 1 {
			return nil, ErrUnlock
		}
	}
	var secret struct {
		Session     dbus.ObjectPath
		Parameters  []byte
		Value       []byte
		ContentType string
	}
	if conn.Object("org.freedesktop.secrets", unlocked[0]).CallWithContext(ctx, "org.freedesktop.Secret.Item.GetSecret", 0, session).Store(&secret) != nil {
		return nil, ErrUnlock
	}
	if len(secret.Value) > 1<<20 {
		clear(secret.Value)
		return nil, ErrFormat
	}
	return secret.Value, nil
}
