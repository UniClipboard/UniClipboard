// Command credentials verifies task-owned secrets against a real Secret Service.
// It never reads the user's credential bus: run it inside dbus-run-session.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/godbus/dbus/v5"

	"github.com/uniclipboard/unic/internal/v1migration"
)

func main() {
	store := flag.Bool("store", false, "store a synthetic binary test credential in the private bus")
	missing := flag.Bool("missing", false, "require an actionable missing-credential error")
	unavailable := flag.Bool("unavailable", false, "require an actionable unavailable-backend error")
	cancelled := flag.Bool("cancelled", false, "require cancellation before accessing the private credential bus")
	legacy := flag.Bool("legacy", false, "use the legacy Secret Service attributes without target")
	ambiguous := flag.Bool("ambiguous", false, "require rejection of two matching task-owned credentials")
	flag.Parse()
	// Includes NUL and non-UTF-8; a text/base64 conversion must not pass.
	secret := []byte{0, 255, 3, 0, 128, 10, 250, 1}
	const service = "unic-migration-e2e-private-bus"
	account := "synthetic-binary"
	if *legacy {
		account = "synthetic-legacy"
	}
	if *ambiguous {
		account = "synthetic-ambiguous"
	}
	if *store {
		must(storeSecret(service, account, secret, *legacy, !*ambiguous))
		if *ambiguous {
			must(storeSecret(service, account, secret, *legacy, false))
		}
		fmt.Println("stored task-owned binary credential")
		return
	}
	if *cancelled {
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		value, err := v1migration.ReadSecretContext(ctx, service, account)
		clear(value)
		if !errors.Is(err, context.Canceled) {
			panic(fmt.Sprintf("expected context cancellation, got %v", err))
		}
		fmt.Println("cancelled before credential access")
		return
	}
	name := account
	if *missing {
		name = "synthetic-missing"
	}
	value, err := v1migration.ReadSecret(service, name)
	defer clear(value)
	if *missing || *unavailable || *ambiguous {
		want := "credential was not found"
		if *ambiguous {
			want = "ambiguous"
		}
		if *unavailable {
			want = "Secret Service is unavailable"
		}
		if err == nil || !bytes.Contains([]byte(err.Error()), []byte(want)) {
			panic(fmt.Sprintf("expected actionable credential failure %q, got %v", want, err))
		}
		must(json.NewEncoder(os.Stdout).Encode(map[string]any{"status": "passed", "error": err.Error()}))
		return
	}
	must(err)
	if !bytes.Equal(value, secret) {
		panic("raw binary secret changed")
	}
	fmt.Println("read exact task-owned binary credential")
}

func must(err error) {
	if err != nil {
		panic(err)
	}
}

func storeSecret(service, account string, value []byte, legacy, replace bool) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	c, err := dbus.ConnectSessionBus()
	if err != nil {
		return err
	}
	defer func() { _ = c.Close() }()
	object := c.Object("org.freedesktop.secrets", "/org/freedesktop/secrets")
	var output dbus.Variant
	var session dbus.ObjectPath
	if err = object.CallWithContext(ctx, "org.freedesktop.Secret.Service.OpenSession", 0, "plain", dbus.MakeVariant("")).Store(&output, &session); err != nil {
		return err
	}
	defer c.Object("org.freedesktop.secrets", session).Call("org.freedesktop.Secret.Session.Close", 0)
	var collection dbus.ObjectPath
	if err = object.CallWithContext(ctx, "org.freedesktop.Secret.Service.ReadAlias", 0, "default").Store(&collection); err != nil {
		return err
	}
	if collection == "/" {
		return fmt.Errorf("private keyring must have an unlocked default collection")
	}
	attributes := map[string]string{"service": service, "username": account}
	if !legacy {
		attributes["target"] = "default"
	}
	properties := map[string]dbus.Variant{
		"org.freedesktop.Secret.Item.Label":      dbus.MakeVariant("UniClipboard synthetic migration acceptance"),
		"org.freedesktop.Secret.Item.Attributes": dbus.MakeVariant(attributes),
	}
	secret := struct {
		Session     dbus.ObjectPath
		Parameters  []byte
		Value       []byte
		ContentType string
	}{session, []byte{}, value, "application/octet-stream"}
	var item, prompt dbus.ObjectPath
	if err = c.Object("org.freedesktop.secrets", collection).CallWithContext(ctx, "org.freedesktop.Secret.Collection.CreateItem", 0, properties, secret, replace).Store(&item, &prompt); err != nil {
		return err
	}
	if prompt != "/" {
		return fmt.Errorf("test store unexpectedly requires a prompt")
	}
	return nil
}
