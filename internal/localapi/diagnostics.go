package api

import (
	"context"

	"github.com/uniclipboard/unic/internal/observability"
)

func (c *Client) LogLevels(ctx context.Context) (v observability.LogLevelState, err error) {
	err = c.do(ctx, "GET", "/v1/diagnostics/log-level", nil, &v)
	return
}
func (c *Client) SetLogLevel(ctx context.Context, target, level string, seconds int) (v observability.LogLevelState, err error) {
	err = c.do(ctx, "PUT", "/v1/diagnostics/log-level", map[string]any{"target": target, "level": level, "duration_seconds": seconds}, &v)
	return
}
func (c *Client) ResetLogLevel(ctx context.Context, target string) (v observability.LogLevelState, err error) {
	err = c.do(ctx, "DELETE", "/v1/diagnostics/log-level?target="+target, nil, &v)
	return
}
func (c *Client) Telemetry(ctx context.Context) (v observability.TelemetryState, err error) {
	err = c.do(ctx, "GET", "/v1/diagnostics/telemetry", nil, &v)
	return
}
