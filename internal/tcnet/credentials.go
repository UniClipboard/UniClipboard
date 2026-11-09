package tcnet

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
)

// ErrBadCredentials means the credentials file exists but cannot be used:
// wrong type or permissions, damaged, or incomplete. It is not repaired and
// not replaced.
var ErrBadCredentials = errors.New("tailcat credentials are unusable")

const credFile = "tailcat.json"

// credentials are everything that makes this device the same tailcat endpoint
// after a restart. They are secrets; the address embeds the pre-shared key.
type credentials struct {
	Version   int    `json:"version"`
	ServerKey string `json:"server_key"`
	PSK       string `json:"psk"`
	ClientKey string `json:"client_key"`
	Addr      string `json:"addr,omitempty"` // known after the first Start
}

func newCredentials() (credentials, error) {
	sk, err := key.NewNode().MarshalText()
	if err != nil {
		return credentials{}, err
	}
	ck, err := key.NewNode().MarshalText()
	if err != nil {
		return credentials{}, err
	}
	pk, err := tailcat.NewPresharedKey().MarshalText()
	if err != nil {
		return credentials{}, err
	}
	return credentials{Version: 1, ServerKey: string(sk), PSK: string(pk), ClientKey: string(ck)}, nil
}

// loadCredentials reads dir/tailcat.json, creating it only when it is absent.
func loadCredentials(dir string) (credentials, error) {
	if fi, err := os.Lstat(dir); err != nil || !fi.IsDir() || fi.Mode()&fs.ModeSymlink != 0 {
		return credentials{}, ErrBadCredentials
	}
	path := filepath.Join(dir, credFile)
	fi, err := os.Lstat(path)
	if errors.Is(err, fs.ErrNotExist) {
		c, err := newCredentials()
		if err != nil {
			return credentials{}, err
		}
		if err := saveCredentials(dir, c); err != nil {
			return credentials{}, err
		}
		return c, nil
	}
	if err != nil {
		return credentials{}, err
	}
	if !fi.Mode().IsRegular() || (runtime.GOOS != "windows" && fi.Mode().Perm()&0o077 != 0) {
		return credentials{}, ErrBadCredentials
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return credentials{}, err
	}
	var c credentials
	if json.Unmarshal(raw, &c) != nil || c.Version != 1 || c.ServerKey == "" || c.PSK == "" || c.ClientKey == "" {
		return credentials{}, ErrBadCredentials
	}
	return c, nil
}

// saveCredentials publishes c whole: temporary file, fsync, rename, directory
// sync. A crash leaves the previous complete file or the new one.
func saveCredentials(dir string, c credentials) error {
	data, err := json.Marshal(c)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".tailcat.json.*")
	if err != nil {
		return fmt.Errorf("tailcat credentials: %w", err)
	}
	name := tmp.Name()
	ok := false
	defer func() {
		if !ok {
			_ = os.Remove(name)
		}
	}()
	if err := tmp.Chmod(0o600); err != nil && runtime.GOOS != "windows" {
		_ = tmp.Close()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Rename(name, filepath.Join(dir, credFile)); err != nil {
		return err
	}
	ok = true
	if d, err := os.Open(dir); err == nil {
		_ = d.Sync()
		_ = d.Close()
	}
	return nil
}
