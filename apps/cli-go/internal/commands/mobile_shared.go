package commands

import (
	"bufio"
	"context"
	"errors"
	"io"
	"net/http"
	"os"
	"strings"
	"unicode/utf8"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// mobileSession is a daemon client plus the control lease that keeps a
// oneshot daemon alive for the duration of one mobile command.
type mobileSession struct {
	client *daemonclient.Client
	lease  *daemonclient.Lease
}

func (s *mobileSession) close() { s.lease.Release() }

// enterMobile prints the optional header (human mode only), then connects to
// or spawns a daemon and holds its control lease. On failure the error was
// already reported and the exit code is returned.
func enterMobile(header string, asJSON bool) (*mobileSession, int) {
	if !asJSON && header != "" {
		ui.Header(header)
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return nil, session.ExitCode(err)
	}
	return &mobileSession{client: client, lease: lease}, 0
}

// updateMobileSyncSettingsRequest is the PATCH /mobile-sync/settings body.
// The double pointers carry the three wire states: nil = field absent (leave
// unchanged), pointer to nil = explicit null (clear), value = set.
type updateMobileSyncSettingsRequest struct {
	Enabled             *bool    `json:"enabled"`
	LanListenEnabled    *bool    `json:"lanListenEnabled"`
	LanAdvertiseIP      **string `json:"lanAdvertiseIp,omitempty"`
	LanPort             **uint16 `json:"lanPort,omitempty"`
	LanAdvertiseBaseURL **string `json:"lanAdvertiseBaseUrl,omitempty"`
}

type updateMobileSyncSettingsResult struct {
	Enabled              bool    `json:"enabled"`
	LanListenEnabled     bool    `json:"lanListenEnabled"`
	LanAdvertiseIP       *string `json:"lanAdvertiseIp"`
	LanPort              *uint16 `json:"lanPort"`
	LanAdvertiseBaseURL  *string `json:"lanAdvertiseBaseUrl"`
	RestartRequired      bool    `json:"restartRequired"`
	LanListenerBindError *string `json:"lanListenerBindError"`
}

type registerMobileDeviceRequest struct {
	Label    string  `json:"label"`
	Username *string `json:"username"`
	Password *string `json:"password"`
}

type registerMobileDeviceResult struct {
	DeviceID    string `json:"deviceId"`
	Label       string `json:"label"`
	BaseURL     string `json:"baseUrl"`
	Username    string `json:"username"`
	Password    string `json:"password"`
	InstallURL  string `json:"installUrl"`
	QRCodeASCII string `json:"qrCodeAscii"`
}

type mobileDeviceView struct {
	DeviceID     string `json:"deviceId"`
	Label        string `json:"label"`
	LastSeenAtMs *int64 `json:"lastSeenAtMs"`
}

func boolPtr(b bool) *bool { return &b }

func (s *mobileSession) updateSettings(req updateMobileSyncSettingsRequest) (updateMobileSyncSettingsResult, error) {
	var out updateMobileSyncSettingsResult
	err := s.client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPatch, Path: "/mobile-sync/settings", JSON: req,
	}, &out)
	return out, err
}

func (s *mobileSession) registerDevice(req registerMobileDeviceRequest) (registerMobileDeviceResult, error) {
	var out registerMobileDeviceResult
	err := s.client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost, Path: "/mobile-sync/devices", JSON: req,
	}, &out)
	return out, err
}

func (s *mobileSession) listDevices() ([]mobileDeviceView, error) {
	var out []mobileDeviceView
	err := s.client.Get(context.Background(), "/mobile-sync/devices", &out)
	return out, err
}

// readPasswordStdin reads one line of stdin like Rust's `read_line`: EOF
// without a newline is not an error, and only the line terminator is trimmed.
func readPasswordStdin() (string, error) {
	line, err := bufio.NewReader(os.Stdin).ReadBytes('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	if !utf8.Valid(line) {
		return "", errors.New("stream did not contain valid UTF-8")
	}
	return strings.TrimRight(string(line), "\n\r"), nil
}

// mobileRestartHint turns `restart_required=true` into a user-facing hint.
const mobileRestartHint = "Restart the daemon to apply: `uniclip stop && uniclip start`."

// confirmNetworkRisk prints the LAN exposure warning shared by `setup` and
// `network set` and asks for acceptance; a prompt failure counts as "no".
func confirmNetworkRisk() bool {
	ui.Warn("Enabling LAN listener exposes clipboard data over your local network.")
	ui.Info("•", "Body is unencrypted in v1 (HTTPS comes in v2).")
	ui.Info("•", "Only enable on trusted networks (home / private office).")
	ui.Info("•", "Strongly discouraged on public WiFi.")
	ui.Info("•", "Anyone on the same LAN can sniff your data.")
	accepted, err := ui.Confirm("Accept network exposure and continue?", false)
	if err != nil || !accepted {
		ui.Warn("Aborted by user.")
		return false
	}
	return true
}

// printRegistration renders a freshly registered device: credential lines on
// stderr, then the daemon-rendered ASCII QR on stdout framed by blank lines.
func printRegistration(reg registerMobileDeviceResult) {
	ui.Success("Registered device: " + reg.Label)
	ui.Info("deviceId", reg.DeviceID)
	ui.Info("baseUrl", reg.BaseURL)
	ui.Info("username", reg.Username)
	ui.Info("password (one-time)", reg.Password)
	ui.Info("installUrl", reg.InstallURL)
	ui.Bar()
	os.Stdout.WriteString("\n" + reg.QRCodeASCII + "\n\n")
	ui.Info("next", "Scan the QR with iPhone Camera, install the SyncClipboard shortcut, then edit url / username / password fields.")
}
