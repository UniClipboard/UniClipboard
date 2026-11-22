package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
)

type PluginActionInfo struct {
	ID          string   `json:"id"`
	Key         string   `json:"key"`
	Title       string   `json:"title"`
	Accepts     []string `json:"accepts,omitempty"`
	Placements  []string `json:"placements,omitempty"`
	PostActions []string `json:"post_actions,omitempty"`
	TimeoutMS   int64    `json:"timeout_ms,omitempty"`
}

type PluginInfo struct {
	ImplementationID string             `json:"implementation_id"`
	Capabilities     []string           `json:"capabilities"`
	TransportKind    string             `json:"transport_kind,omitempty"`
	ExternalChannels bool               `json:"external_channels,omitempty"`
	ID               string             `json:"id"`
	Version          string             `json:"version"`
	Kind             string             `json:"kind"`
	Source           string             `json:"source"`
	Repository       string             `json:"repository,omitempty"`
	Commit           string             `json:"commit,omitempty"`
	HasPrevious      bool               `json:"has_previous,omitempty"`
	PreviousVersion  string             `json:"previous_version,omitempty"`
	Events           []string           `json:"events,omitempty"`
	Actions          []PluginActionInfo `json:"actions,omitempty"`
	Configured       bool               `json:"configured_enabled"`
	RunningVersion   string             `json:"running_version,omitempty"`
	HasSettings      bool               `json:"has_settings"`
	Restart          bool               `json:"restart_required"`
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

type MarketplaceEntry struct {
	ID            string   `json:"id"`
	Name          string   `json:"name"`
	Author        string   `json:"author"`
	Official      bool     `json:"official,omitempty"`
	Category      string   `json:"category"`
	Version       string   `json:"version"`
	Summary       string   `json:"summary"`
	Description   string   `json:"description,omitempty"`
	Highlights    []string `json:"highlights,omitempty"`
	HowTo         []string `json:"how_to,omitempty"`
	Privacy       string   `json:"privacy,omitempty"`
	ZeroConfig    bool     `json:"zero_config,omitempty"`
	Repository    string   `json:"repository"`
	Ref           string   `json:"ref,omitempty"`
	Path          string   `json:"path,omitempty"`
	BuildFallback bool     `json:"build_fallback,omitempty"`
	Capabilities  []string `json:"capabilities,omitempty"`
}

type PluginMarketplace struct {
	Version   int                `json:"version"`
	Source    string             `json:"source"`
	UpdatedAt string             `json:"updated_at,omitempty"`
	Warning   string             `json:"warning,omitempty"`
	Plugins   []MarketplaceEntry `json:"plugins"`
}

type PluginInstallRequest struct {
	Repository string `json:"repository"`
	Ref        string `json:"ref,omitempty"`
	Path       string `json:"path,omitempty"`
	Build      bool   `json:"build,omitempty"`
	Replace    bool   `json:"replace,omitempty"`
	Enable     bool   `json:"enable,omitempty"`
}

func (c *Client) Plugins(ctx context.Context) (out []PluginInfo, err error) {
	err = c.do(ctx, http.MethodGet, "/v1/plugins", nil, &out)
	return
}

func (c *Client) PluginMarketplace(ctx context.Context, refresh bool) (out PluginMarketplace, err error) {
	path := "/v1/plugins/marketplace"
	if refresh {
		path += "?refresh=1"
	}
	err = c.do(ctx, http.MethodGet, path, nil, &out)
	return
}

func (c *Client) InstallPlugin(ctx context.Context, in PluginInstallRequest) (out PluginInfo, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/plugins/install", in, &out)
	return
}

func (c *Client) UninstallPlugin(ctx context.Context, id string) error {
	return c.do(ctx, http.MethodDelete, "/v1/plugins/"+url.PathEscape(id), nil, nil)
}

func (c *Client) RollbackPlugin(ctx context.Context, id string) (out PluginInfo, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/plugins/"+url.PathEscape(id)+"/rollback", nil, &out)
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

type PluginInstanceCreate struct {
	ID               string `json:"id"`
	ImplementationID string `json:"implementation_id"`
	Revision         string `json:"revision"`
}

func (c *Client) CreatePluginInstance(ctx context.Context, in PluginInstanceCreate) (out PluginInfo, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/plugin-instances", in, &out)
	return
}
