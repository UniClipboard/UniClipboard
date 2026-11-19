package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
)

type PluginInfo struct {
	ID             string `json:"id"`
	Version        string `json:"version"`
	Kind           string `json:"kind"`
	Source         string `json:"source"`
	Configured     bool   `json:"configured_enabled"`
	RunningVersion string `json:"running_version,omitempty"`
	HasSettings    bool   `json:"has_settings"`
	Restart        bool   `json:"restart_required"`
}

type PluginSettings struct {
	Revision string          `json:"revision"`
	Schema   json.RawMessage `json:"schema"`
	Values   json.RawMessage `json:"values"`
	Secrets  map[string]bool `json:"secrets"`
	Restart  bool            `json:"restart_required"`
}

type PluginSettingsEdit struct {
	Revision string          `json:"revision"`
	Values   json.RawMessage `json:"values"`
}

func (c *Client) Plugins(ctx context.Context) (out []PluginInfo, err error) {
	err = c.do(ctx, http.MethodGet, "/v1/plugins", nil, &out)
	return
}

func (c *Client) PluginSettings(ctx context.Context, id string) (out PluginSettings, err error) {
	err = c.do(ctx, http.MethodGet, "/v1/plugins/"+url.PathEscape(id)+"/settings", nil, &out)
	return
}

func (c *Client) SavePluginSettings(ctx context.Context, id string, in PluginSettingsEdit) (out PluginSettings, err error) {
	err = c.do(ctx, http.MethodPut, "/v1/plugins/"+url.PathEscape(id)+"/settings", in, &out)
	return
}
