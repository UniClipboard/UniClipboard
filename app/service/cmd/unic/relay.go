package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const relayUsage = `Configure and authorize UniClipboard Hybrid Relay.

Usage:
  unic relay login [flags]

Commands:
  login                 Sign in to UniClipboard Cloud and automatically save a Relay API key

Flags:
  --cloud-url URL       Override UniClipboard Cloud origin (default $UNIC_CLOUD_URL or https://cloud.uniclipboard.app)
  --no-browser          Print the authorization URL instead of launching the system browser
  --json                Output result as JSON (never includes the raw API key)
  -h, --help            Show this help

Examples:
  unic relay login
  unic relay login --no-browser
`

func runRelayCmd(ctx context.Context, sub string, args []string, asJSON bool) error {
	switch sub {
	case "login":
		return runRelayLogin(ctx, args, os.Stdout, os.Stderr, asJSON, cloudauth.LoginForRelayKey)
	default:
		return usageError(formatUsageLine(relayUsage))
	}
}

func runRelayLogin(
	ctx context.Context,
	args []string,
	stdout, stderr io.Writer,
	asJSON bool,
	loginFn func(context.Context, cloudauth.LoginOptions) (cloudauth.RelayCredential, error),
) error {
	var cloudURL string
	var noBrowser bool
	for i := 0; i < len(args); i++ {
		switch a := args[i]; {
		case a == "--json":
			asJSON = true
		case a == "--no-browser":
			noBrowser = true
		case a == "--cloud-url":
			if i+1 >= len(args) {
				return usageError("flag needs an argument: --cloud-url")
			}
			i++
			cloudURL = args[i]
		case strings.HasPrefix(a, "--cloud-url="):
			cloudURL = strings.TrimPrefix(a, "--cloud-url=")
		default:
			return usageError("usage: unic relay login [--cloud-url URL] [--no-browser] [--json]")
		}
	}

	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	cfgPath := filepath.Join(dir, "config.toml")
	mgr, err := config.Open(cfgPath, nil)
	if err != nil {
		return err
	}
	cur := mgr.Current()
	st := mgr.State()

	loginCtx, cancel := context.WithTimeout(ctx, 5*time.Minute)
	defer cancel()

	cred, err := loginFn(loginCtx, cloudauth.LoginOptions{
		CloudURL:   cloudURL,
		DeviceName: cur.DeviceName,
		OpenBrowser: func(authURL string) error {
			if noBrowser {
				_, _ = fmt.Fprintf(stderr, "Open this URL in your browser to authorize UniClipboard:\n  %s\n", authURL)
				return nil
			}
			if berr := cloudauth.OpenSystemBrowser(authURL); berr != nil {
				_, _ = fmt.Fprintf(stderr, "Could not open browser automatically. Open this URL manually:\n  %s\n", authURL)
				return nil
			}
			if !asJSON {
				_, _ = fmt.Fprintln(stderr, "Waiting for browser authorization in UniClipboard Cloud...")
			}
			return nil
		},
	})
	if err != nil {
		return err
	}

	next := cur
	next.Sync.Relay.Enabled = true
	next.Sync.Relay.AccessKey = cred.APIKey
	if cred.RelayURL == "" || strings.EqualFold(strings.TrimRight(cred.RelayURL, "/"), relayclient.DefaultURL) {
		next.Sync.Relay.URL = ""
	} else if norm, nerr := relaycrypto.NormalizeRelayOrigin(cred.RelayURL); nerr == nil {
		next.Sync.Relay.URL = norm
	}

	var buf bytes.Buffer
	if err := toml.NewEncoder(&buf).Encode(next); err != nil {
		return err
	}
	if _, err := mgr.Replace(st.Revision, buf.Bytes()); err != nil {
		return err
	}

	effectiveRelay := next.Sync.Relay.URL
	if effectiveRelay == "" {
		effectiveRelay = relayclient.DefaultURL
	}
	if asJSON {
		return json.NewEncoder(stdout).Encode(map[string]any{
			"authorized": true,
			"enabled":    true,
			"relay_url":  effectiveRelay,
			"key_prefix": cred.KeyPrefix,
			"config":     cfgPath,
		})
	}
	_, _ = fmt.Fprintf(stdout, "Authorized with UniClipboard Cloud (%s). Hybrid Relay enabled for %s.\n", cred.KeyPrefix, effectiveRelay)
	return nil
}
