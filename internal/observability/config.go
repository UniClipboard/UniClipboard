// Package observability owns bounded, configurable client diagnostics. Business
// callers only enqueue fixed data; workers own all file and network I/O.
package observability

import (
	"errors"
	"log/slog"
	"net/url"
	"strconv"
	"strings"
	"time"
)

type Logging struct {
	Level string `toml:"level" json:"level"`
}
type Logs struct {
	Enabled bool   `toml:"enabled" json:"enabled"`
	Level   string `toml:"level" json:"level"`
}
type Metrics struct {
	Enabled bool `toml:"enabled" json:"enabled"`
}
type Analytics struct {
	Enabled bool `toml:"enabled" json:"enabled"`
}
type Telemetry struct {
	Enabled   bool      `toml:"enabled" json:"enabled"`
	Endpoint  string    `toml:"endpoint" json:"endpoint"`
	Logs      Logs      `toml:"logs" json:"logs"`
	Metrics   Metrics   `toml:"metrics" json:"metrics"`
	Analytics Analytics `toml:"analytics" json:"analytics"`
}
type Config struct {
	Logging   Logging
	Telemetry Telemetry
}

func Default() Config {
	return Config{Logging: Logging{"info"}, Telemetry: Telemetry{Enabled: true, Endpoint: releaseEndpoint(), Logs: Logs{Enabled: true, Level: "warn"}, Metrics: Metrics{Enabled: true}, Analytics: Analytics{Enabled: true}}}
}
func Level(s string) (slog.Level, bool) {
	switch s {
	case "debug":
		return slog.LevelDebug, true
	case "info":
		return slog.LevelInfo, true
	case "warn":
		return slog.LevelWarn, true
	case "error":
		return slog.LevelError, true
	}
	return 0, false
}
func (c Config) Validate() error {
	if _, ok := Level(c.Logging.Level); !ok {
		return errors.New("logging.level: expected debug, info, warn or error")
	}
	if _, ok := Level(c.Telemetry.Logs.Level); !ok {
		return errors.New("telemetry.logs.level: expected debug, info, warn or error")
	}
	if c.Telemetry.Endpoint == "" {
		return nil
	}
	u, err := url.Parse(c.Telemetry.Endpoint)
	if err != nil || u.Scheme != "https" || u.Hostname() == "" || u.User != nil || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || (u.Path != "" && u.Path != "/" && u.Path != "/i" && u.Path != "/i/") || u.RawPath != "" || strings.ContainsAny(c.Telemetry.Endpoint, "\r\n#?") {
		return errors.New("telemetry.endpoint: expected an HTTPS base URL with optional /i base path and without credentials, query or fragment")
	}
	if port := u.Port(); port != "" {
		n, err := strconv.Atoi(port)
		if err != nil || n < 1 || n > 65535 {
			return errors.New("telemetry.endpoint: invalid port")
		}
	}
	return nil
}

type LevelState struct {
	Configured string `json:"configured_level"`
	Effective  string `json:"effective_level"`
	Source     string `json:"source"`
	Expires    *int64 `json:"expires_at_ms"`
	Dropped    uint64 `json:"dropped_records"`
}
type LogLevelState struct {
	Local         LevelState `json:"local"`
	Remote        LevelState `json:"remote"`
	Enabled       bool       `json:"telemetry_enabled"`
	RemoteEnabled bool       `json:"remote_logs_enabled"`
}
type SignalState struct {
	Requested bool    `json:"requested_enabled"`
	State     string  `json:"state"`
	Error     *string `json:"last_error_code"`
	Dropped   uint64  `json:"dropped_records"`
	Failures  uint64  `json:"export_failures"`
}
type AnalyticsState struct {
	SignalState
	Pending int64 `json:"pending_records"`
}
type TelemetryState struct {
	Configured bool           `json:"configured_enabled"`
	Effective  bool           `json:"effective_enabled"`
	Restart    []string       `json:"restart_required"`
	Credential bool           `json:"credential_configured"`
	Logs       SignalState    `json:"logs"`
	Metrics    SignalState    `json:"metrics"`
	Analytics  AnalyticsState `json:"analytics"`
}
type override struct {
	level    string
	deadline time.Time
	timer    *time.Timer
}
