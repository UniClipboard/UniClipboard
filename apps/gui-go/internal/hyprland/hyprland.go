// Package hyprland is a bounded client for the Hyprland IPC socket, used to remember the window that had the focus
// before the quick panel opened and to paste into it. It follows the Tauri shell's `uc_desktop::hyprland`
// (crates/uc-desktop/src/hyprland.rs): the same socket, the same commands, the same identity checks and deadlines.
// The package has no build tags so its protocol handling can be exercised on any host against a scripted socket.
package hyprland

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	ioTimeout       = 150 * time.Millisecond
	maxResponse     = 1 << 20
	focusDeadline   = 500 * time.Millisecond
	focusPollPeriod = 10 * time.Millisecond
)

// Client talks to one Hyprland instance.
type Client struct{ socket string }

// WindowTarget identifies a window. Compositor metadata can contain user content, so it is never logged.
type WindowTarget struct {
	Address string `json:"address"`
	PID     uint32 `json:"pid"`
	Class   string `json:"class"`
}

// Current returns a client for the running Hyprland instance, or nil when this session is not Hyprland.
func Current() *Client {
	runtime := os.Getenv("XDG_RUNTIME_DIR")
	instance := os.Getenv("HYPRLAND_INSTANCE_SIGNATURE")
	if runtime == "" || instance == "" {
		return nil
	}
	for _, r := range instance {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '_' || r == '-') {
			return nil
		}
	}
	return &Client{socket: filepath.Join(runtime, "hypr", instance, ".socket.sock")}
}

// NewAt is the client for an explicit socket path (used by the contract check).
func NewAt(socket string) *Client { return &Client{socket: socket} }

func (c *Client) request(command string) (string, error) {
	deadline := time.Now().Add(ioTimeout)
	conn, err := net.DialTimeout("unix", c.socket, ioTimeout)
	if err != nil {
		return "", errors.New("cannot connect to Hyprland IPC within the deadline")
	}
	defer conn.Close()
	_ = conn.SetDeadline(deadline)
	if _, err := io.WriteString(conn, command); err != nil {
		return "", errors.New("Hyprland IPC write failed")
	}
	response, err := io.ReadAll(io.LimitReader(conn, maxResponse+1))
	if err != nil {
		return "", errors.New("Hyprland IPC read failed or timed out")
	}
	if len(response) > maxResponse {
		return "", errors.New("Hyprland IPC response exceeded the limit")
	}
	return string(response), nil
}

// ActiveWindow is the focused window, or nil when no window has the focus.
func (c *Client) ActiveWindow() (*WindowTarget, error) {
	response, err := c.request("j/activewindow")
	if err != nil {
		return nil, err
	}
	var object map[string]json.RawMessage
	if err := json.Unmarshal([]byte(response), &object); err != nil {
		return nil, errors.New("invalid Hyprland active-window response")
	}
	if len(object) == 0 {
		return nil, nil
	}
	var target WindowTarget
	if err := json.Unmarshal([]byte(response), &target); err != nil {
		return nil, errors.New("incomplete Hyprland window identity")
	}
	if err := target.Validate(); err != nil {
		return nil, err
	}
	return &target, nil
}

// Validate rejects an identity that could escape the dispatcher argument it is interpolated into.
func (t WindowTarget) Validate() error {
	hex := strings.TrimPrefix(t.Address, "0x")
	if hex == t.Address || t.PID == 0 || hex == "" || len(hex) > 16 || strings.Trim(hex, "0") == "" {
		return errors.New("invalid Hyprland window identity")
	}
	for _, r := range hex {
		if !(r >= '0' && r <= '9' || r >= 'a' && r <= 'f' || r >= 'A' && r <= 'F') {
			return errors.New("invalid Hyprland window identity")
		}
	}
	return nil
}

func (c *Client) dispatch(lua string) error {
	response, err := c.request("/dispatch " + lua)
	if err != nil {
		return err
	}
	if strings.TrimSpace(response) != "ok" {
		// The compositor's reply can include window metadata, so it is not echoed.
		return errors.New("Hyprland rejected the panel input operation")
	}
	return nil
}

// Focus brings the target to the front, after checking it is still the same window (an address can be reused by
// another process), and waits until the compositor confirms it.
func (c *Client) Focus(target WindowTarget) error {
	if err := target.Validate(); err != nil {
		return err
	}
	response, err := c.request("j/clients")
	if err != nil {
		return err
	}
	var clients []WindowTarget
	if err := json.Unmarshal([]byte(response), &clients); err != nil {
		return errors.New("invalid Hyprland client list")
	}
	found := false
	for _, client := range clients {
		if client == target {
			found = true
			break
		}
	}
	if !found {
		return errors.New("the previous application window is no longer available")
	}
	if err := c.dispatch(fmt.Sprintf(`hl.dsp.focus({ window = "address:%s" })`, target.Address)); err != nil {
		return err
	}
	deadline := time.Now().Add(focusDeadline)
	for {
		active, err := c.ActiveWindow()
		if err != nil {
			return err
		}
		if active != nil && *active == target {
			return nil
		}
		if !time.Now().Before(deadline) {
			return errors.New("previous application focus could not be confirmed")
		}
		time.Sleep(focusPollPeriod)
	}
}

// SendPaste sends the paste shortcut to the (already focused) target.
func (c *Client) SendPaste(target WindowTarget) error {
	if err := target.Validate(); err != nil {
		return err
	}
	return c.dispatch(fmt.Sprintf(`hl.dsp.send_shortcut({ mods = "%s", key = "V", window = "address:%s" })`,
		PasteModifiers(target.Class), target.Address))
}

// PasteModifiers is Ctrl+Shift+V for terminal emulators and Ctrl+V for everything else.
func PasteModifiers(class string) string {
	switch strings.ToLower(class) {
	case "alacritty", "kitty", "foot", "footclient", "com.mitchellh.ghostty", "org.wezfurlong.wezterm",
		"org.gnome.terminal", "konsole":
		return "CTRL SHIFT"
	}
	return "CTRL"
}
