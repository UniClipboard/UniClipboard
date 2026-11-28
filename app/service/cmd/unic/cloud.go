package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/relayclient"
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
  --json                Output result as JSON (never includes raw tokens or credentials)
  -h, --help            Show this help

Examples:
  unic cloud login
  unic cloud status
  unic cloud logout
`

type cloudCmdDeps struct {
	loginFn    func(context.Context, cloudauth.LoginOptions) (cloudauth.Session, error)
	httpClient *http.Client
}

func defaultCloudDeps() cloudCmdDeps {
	return cloudCmdDeps{
		loginFn: cloudauth.Login,
	}
}

func (d cloudCmdDeps) newManager(dir, deviceName string) *cloudauth.Manager {
	return cloudauth.NewManager(dir, cloudauth.ManagerOptions{
		HTTPClient: d.httpClient,
		DeviceName: func() string { return deviceName },
	})
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
	cfgMgr, err := config.Open(cfgPath, nil)
	if err != nil {
		return err
	}
	cur := cfgMgr.Current()

	loginCtx, cancel := context.WithTimeout(ctx, 5*time.Minute)
	defer cancel()

	sess, err := deps.loginFn(loginCtx, cloudauth.LoginOptions{
		CloudURL:   cloudURL,
		DeviceName: cur.DeviceName,
		HTTPClient: deps.httpClient,
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

	authMgr := deps.newManager(dir, cur.DeviceName)
	if sess, err = authMgr.SetSession(sess); err != nil {
		return err
	}

	relayEnabled := cur.Sync.Enabled && cur.Sync.Relay.Enabled
	relayURL := strings.TrimSpace(cur.Sync.Relay.URL)
	var cred cloudauth.DeviceCredential
	if !noRelay && strings.TrimSpace(cur.Sync.Relay.URL) == "" {
		cred, sess, err = authMgr.EnsureDeviceCredential(loginCtx)
		if err != nil {
			return err
		}
		if raw, serr := config.Set(cur, "sync.relay.enabled", "true"); serr == nil {
			for attempt := 0; attempt < 3; attempt++ {
				st := cfgMgr.State()
				if _, rerr := cfgMgr.Replace(st.Revision, raw); rerr == nil {
					relayEnabled = true
					break
				} else if rerr != config.ErrConflict {
					return rerr
				}
				_ = cfgMgr.Reload()
				raw, err = config.Set(cfgMgr.Current(), "sync.relay.enabled", "true")
				if err != nil {
					return err
				}
			}
		} else {
			return serr
		}
		if cred.RelayURL != "" {
			if norm, nerr := cloudauth.NormalizeAdvertisedRelayURL(cred.RelayURL); nerr == nil && norm != "" {
				relayURL = norm
			} else {
				relayURL = relayclient.DefaultURL
			}
		} else {
			relayURL = relayclient.DefaultURL
		}
	}

	status := sess.Status(true)
	if asJSON {
		return json.NewEncoder(stdout).Encode(map[string]any{
			"signed_in":                  status.SignedIn,
			"state":                      status.State,
			"cloud_url":                  status.CloudURL,
			"account":                    status.Account,
			"relay_enabled":              relayEnabled,
			"relay_url":                  relayURL,
			"credential_prefix":          status.CredentialPrefix,
			"credential_expires_at_ms":   status.CredentialExpiresAtMs,
			"credential_rotate_after_ms": status.CredentialRotateAfterMs,
		})
	}
	if !noRelay && strings.TrimSpace(cur.Sync.Relay.URL) == "" && relayEnabled {
		_, _ = fmt.Fprintf(stdout, "Signed in to UniClipboard Cloud as %s. Hybrid Relay enabled (%s).\n", status.Account, relayURL)
	} else {
		_, _ = fmt.Fprintf(stdout, "Signed in to UniClipboard Cloud as %s.\n", status.Account)
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
	status, err := cloudauth.NewManager(dir, cloudauth.ManagerOptions{}).Status()
	if err != nil {
		return err
	}
	if asJSON {
		return json.NewEncoder(stdout).Encode(status)
	}
	switch status.State {
	case cloudauth.SessionStateReauthRequired:
		_, _ = fmt.Fprintf(stdout, "UniClipboard Cloud session for %s (%s) expired or was revoked. Run `unic cloud login` to sign in again.\n", status.Account, status.CloudURL)
	case cloudauth.SessionStateActive:
		_, _ = fmt.Fprintf(stdout, "Signed in to UniClipboard Cloud (%s) as %s\n", status.CloudURL, status.Account)
		if status.CredentialPrefix != "" {
			_, _ = fmt.Fprintf(stdout, "Managed relay credential: %s\n", status.CredentialPrefix)
		}
	default:
		_, _ = fmt.Fprintln(stdout, "Not signed in to UniClipboard Cloud. Run `unic cloud login` to sign in.")
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
	revCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	if err := deps.newManager(dir, "").Logout(revCtx); err != nil {
		return err
	}
	if !keepRelay && strings.TrimSpace(os.Getenv("UNIC_RELAY_ACCESS_KEY")) == "" {
		cfgPath := filepath.Join(dir, "config.toml")
		if mgr, oerr := config.Open(cfgPath, nil); oerr == nil {
			for attempt := 0; attempt < 3; attempt++ {
				cur := mgr.Current()
				st := mgr.State()
				// Disable only official Cloud-managed relay when no manual fallback key exists.
				// Never delete user manual sync.relay.access_key or disable self-hosted relays.
				if strings.TrimSpace(cur.Sync.Relay.URL) != "" ||
					strings.TrimSpace(cur.Sync.Relay.AccessKey) != "" ||
					!cur.Sync.Relay.Enabled {
					break
				}
				raw, serr := config.Set(cur, "sync.relay.enabled", "false")
				if serr != nil {
					break
				}
				if _, rerr := mgr.Replace(st.Revision, raw); rerr == nil || rerr != config.ErrConflict {
					break
				}
				_ = mgr.Reload()
			}
		}
	}
	if asJSON {
		return json.NewEncoder(stdout).Encode(map[string]any{"signed_out": true})
	}
	_, _ = fmt.Fprintln(stdout, "Signed out of UniClipboard Cloud.")
	return nil
}
