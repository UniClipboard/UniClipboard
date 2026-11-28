package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
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

const cloudUsage = `Sign in to UniClipboard Cloud and manage Cloud-backed features (such as Hybrid Relay).

Usage:
  unic cloud <command> [flags]

Commands:
  login                 Sign in to UniClipboard Cloud in the browser and unlock Cloud services
  status                Show current UniClipboard Cloud account status
  logout                Sign out of UniClipboard Cloud and clear local session credentials

Flags:
  --cloud-url URL       Override UniClipboard Cloud origin (default $UNIC_CLOUD_URL or https://cloud.uniclipboard.app)
  --no-browser          Print the sign-in URL instead of launching the system browser (login)
  --no-relay            Sign in to Cloud without automatically enabling Hybrid Relay (login)
  --keep-relay          Keep local Hybrid Relay settings after signing out (logout)
  --json                Output result as JSON (never includes raw tokens or API keys)
  -h, --help            Show this help

Examples:
  unic cloud login
  unic cloud status
  unic cloud logout
`

type cloudCmdDeps struct {
	loginFn       func(context.Context, cloudauth.LoginOptions) (cloudauth.Session, error)
	createRelayFn func(context.Context, *http.Client, cloudauth.Session, string) (cloudauth.RelayCredential, cloudauth.Session, error)
	revokeRelayFn func(context.Context, *http.Client, cloudauth.Session, string) error
	logoutFn      func(context.Context, *http.Client, cloudauth.Session) error
}

func defaultCloudDeps() cloudCmdDeps {
	return cloudCmdDeps{
		loginFn:       cloudauth.Login,
		createRelayFn: cloudauth.CreateRelayAPIKey,
		revokeRelayFn: cloudauth.RevokeRelayAPIKey,
		logoutFn:      cloudauth.Logout,
	}
}

func runCloudCmd(ctx context.Context, sub string, args []string, asJSON bool) error {
	deps := defaultCloudDeps()
	switch sub {
	case "login":
		return runCloudLogin(ctx, args, os.Stdout, os.Stderr, asJSON, deps)
	case "status":
		return runCloudStatus(args, os.Stdout, asJSON)
	case "logout":
		return runCloudLogout(ctx, args, os.Stdout, asJSON, deps)
	default:
		return usageError(formatUsageLine(cloudUsage))
	}
}

func runCloudLogin(ctx context.Context, args []string, stdout, stderr io.Writer, asJSON bool, deps cloudCmdDeps) error {
	var cloudURL string
	var noBrowser, noRelay bool
	for i := 0; i < len(args); i++ {
		switch a := args[i]; {
		case a == "--json":
			asJSON = true
		case a == "--no-browser":
			noBrowser = true
		case a == "--no-relay":
			noRelay = true
		case a == "--cloud-url":
			if i+1 >= len(args) {
				return usageError("flag needs an argument: --cloud-url")
			}
			i++
			cloudURL = args[i]
		case strings.HasPrefix(a, "--cloud-url="):
			cloudURL = strings.TrimPrefix(a, "--cloud-url=")
		default:
			return usageError("usage: unic cloud login [--cloud-url URL] [--no-browser] [--no-relay] [--json]")
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

	sess, err := deps.loginFn(loginCtx, cloudauth.LoginOptions{
		CloudURL:   cloudURL,
		DeviceName: cur.DeviceName,
		OpenBrowser: func(authURL string) error {
			if noBrowser {
				_, _ = fmt.Fprintf(stderr, "Open this URL in your browser to sign in to UniClipboard Cloud:\n  %s\n", authURL)
				return nil
			}
			if berr := cloudauth.OpenSystemBrowser(authURL); berr != nil {
				_, _ = fmt.Fprintf(stderr, "Could not open browser automatically. Open this URL manually:\n  %s\n", authURL)
				return nil
			}
			if !asJSON {
				_, _ = fmt.Fprintln(stderr, "Waiting for browser sign-in in UniClipboard Cloud...")
			}
			return nil
		},
	})
	if err != nil {
		return err
	}

	relayEnabled := false
	relayURL := ""
	if !noRelay {
		cred, updated, rerr := deps.createRelayFn(loginCtx, nil, sess, cur.DeviceName)
		if rerr != nil {
			_ = cloudauth.SaveSession(dir, sess)
			return rerr
		}
		sess = updated
		next := cur
		next.Sync.Relay.Enabled = true
		next.Sync.Relay.AccessKey = cred.APIKey
		if cred.RelayURL == "" || strings.EqualFold(strings.TrimRight(cred.RelayURL, "/"), relayclient.DefaultURL) {
			next.Sync.Relay.URL = ""
			relayURL = relayclient.DefaultURL
		} else if norm, nerr := relaycrypto.NormalizeRelayOrigin(cred.RelayURL); nerr == nil {
			next.Sync.Relay.URL = norm
			relayURL = norm
		}
		var buf bytes.Buffer
		if err := toml.NewEncoder(&buf).Encode(next); err != nil {
			return err
		}
		if _, err := mgr.Replace(st.Revision, buf.Bytes()); err != nil {
			return err
		}
		relayEnabled = true
	}

	if err := cloudauth.SaveSession(dir, sess); err != nil {
		return err
	}

	if asJSON {
		return json.NewEncoder(stdout).Encode(map[string]any{
			"signed_in":        true,
			"cloud_url":        sess.CloudURL,
			"account":          sess.User.DisplayLabel(),
			"relay_enabled":    relayEnabled,
			"relay_url":        relayURL,
			"relay_key_prefix": sess.RelayKeyPrefix,
		})
	}
	if relayEnabled {
		_, _ = fmt.Fprintf(stdout, "Signed in to UniClipboard Cloud as %s. Hybrid Relay enabled (%s).\n", sess.User.DisplayLabel(), relayURL)
	} else {
		_, _ = fmt.Fprintf(stdout, "Signed in to UniClipboard Cloud as %s.\n", sess.User.DisplayLabel())
	}
	return nil
}

func runCloudStatus(args []string, stdout io.Writer, asJSON bool) error {
	for _, a := range args {
		if a == "--json" {
			asJSON = true
		} else {
			return usageError("usage: unic cloud status [--json]")
		}
	}
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	sess, ok, err := cloudauth.LoadSession(dir)
	if err != nil {
		return err
	}
	if asJSON {
		out := map[string]any{"signed_in": ok}
		if ok {
			out["cloud_url"] = sess.CloudURL
			out["account"] = sess.User.DisplayLabel()
			out["relay_key_prefix"] = sess.RelayKeyPrefix
			out["signed_in_at_ms"] = sess.SignedInAtMs
		}
		return json.NewEncoder(stdout).Encode(out)
	}
	if !ok {
		_, _ = fmt.Fprintln(stdout, "Not signed in to UniClipboard Cloud. Run `unic cloud login` to sign in.")
		return nil
	}
	_, _ = fmt.Fprintf(stdout, "Signed in to UniClipboard Cloud (%s) as %s\n", sess.CloudURL, sess.User.DisplayLabel())
	if sess.RelayKeyPrefix != "" {
		_, _ = fmt.Fprintf(stdout, "Hybrid Relay key: %s\n", sess.RelayKeyPrefix)
	}
	return nil
}

func runCloudLogout(ctx context.Context, args []string, stdout io.Writer, asJSON bool, deps cloudCmdDeps) error {
	var keepRelay bool
	for _, a := range args {
		switch a {
		case "--json":
			asJSON = true
		case "--keep-relay":
			keepRelay = true
		default:
			return usageError("usage: unic cloud logout [--keep-relay] [--json]")
		}
	}
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	sess, ok, err := cloudauth.LoadSession(dir)
	if err != nil {
		return err
	}
	if ok {
		revCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		if !keepRelay && sess.RelayKeyID != "" {
			_ = deps.revokeRelayFn(revCtx, nil, sess, sess.RelayKeyID)
		}
		_ = deps.logoutFn(revCtx, nil, sess)
		if err := cloudauth.DeleteSession(dir); err != nil {
			return err
		}
	}
	if !keepRelay {
		cfgPath := filepath.Join(dir, "config.toml")
		if mgr, oerr := config.Open(cfgPath, nil); oerr == nil {
			cur := mgr.Current()
			st := mgr.State()
			if cur.Sync.Relay.URL == "" && (cur.Sync.Relay.Enabled || cur.Sync.Relay.AccessKey != "") {
				next := cur
				next.Sync.Relay.Enabled = false
				next.Sync.Relay.AccessKey = ""
				if raw, serr := config.Set(next, "sync.relay.access_key", `""`); serr == nil {
					_, _ = mgr.Replace(st.Revision, raw)
				}
			}
		}
	}
	if asJSON {
		return json.NewEncoder(stdout).Encode(map[string]any{"signed_out": true})
	}
	_, _ = fmt.Fprintln(stdout, "Signed out of UniClipboard Cloud.")
	return nil
}
