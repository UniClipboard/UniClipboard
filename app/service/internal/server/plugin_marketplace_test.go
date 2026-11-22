package server

import (
	"context"
	"encoding/json"
	"testing"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type fakePlugins struct {
	fake
	refreshed    bool
	installReq   api.PluginInstallRequest
	uninstallID  string
	rollbackID   string
	marketplace  api.PluginMarketplace
	installedRes api.PluginInfo
}

func (f *fakePlugins) Plugins(context.Context) ([]api.PluginInfo, error) {
	return []api.PluginInfo{}, nil
}
func (f *fakePlugins) PluginSettings(context.Context, string) (api.PluginSettings, error) {
	return api.PluginSettings{}, nil
}
func (f *fakePlugins) SavePluginSettings(context.Context, string, api.PluginSettingsEdit) (api.PluginSettings, error) {
	return api.PluginSettings{}, nil
}
func (f *fakePlugins) PluginMarketplace(_ context.Context, refresh bool) (api.PluginMarketplace, error) {
	f.refreshed = refresh
	return f.marketplace, f.err
}
func (f *fakePlugins) InstallPlugin(_ context.Context, in api.PluginInstallRequest) (api.PluginInfo, error) {
	f.installReq = in
	return f.installedRes, f.err
}
func (f *fakePlugins) UninstallPlugin(_ context.Context, id string) error {
	f.uninstallID = id
	return f.err
}
func (f *fakePlugins) RollbackPlugin(_ context.Context, id string) (api.PluginInfo, error) {
	f.rollbackID = id
	return f.installedRes, f.err
}

func TestPluginMarketplaceAndLifecycleRoutes(t *testing.T) {
	f := &fakePlugins{
		marketplace: api.PluginMarketplace{
			Version: 1,
			Source:  "remote",
			Plugins: []api.MarketplaceEntry{{ID: "unic.wenshushu", Name: "文叔叔临时分享"}},
		},
		installedRes: api.PluginInfo{ID: "unic.wenshushu", Version: "0.1.0"},
	}
	h := Handler(f, config.Static(config.Default()))

	rec := do(t, h, "GET", "/v1/plugins/marketplace?refresh=1", "")
	if rec.Code != 200 || !f.refreshed {
		t.Fatalf("marketplace = %d refreshed=%v body=%s", rec.Code, f.refreshed, rec.Body)
	}
	var cat api.PluginMarketplace
	if err := json.Unmarshal(rec.Body.Bytes(), &cat); err != nil || len(cat.Plugins) != 1 || cat.Plugins[0].ID != "unic.wenshushu" {
		t.Fatalf("bad marketplace response: %v %+v", err, cat)
	}

	// Reject malformed / empty install requests.
	for _, body := range []string{"", "{}", `{"repository":"https://github.com/a/b","unknown":1}`} {
		if r := do(t, h, "POST", "/v1/plugins/install", body); r.Code != 400 {
			t.Fatalf("expected 400 for %q, got %d", body, r.Code)
		}
	}

	rec = do(t, h, "POST", "/v1/plugins/install", `{"repository":"https://github.com/UniClipboard/unic-plugin-wenshushu","ref":"v0.1.0","build":true,"enable":true}`)
	if rec.Code != 200 || f.installReq.Repository != "https://github.com/UniClipboard/unic-plugin-wenshushu" || !f.installReq.Build || !f.installReq.Enable {
		t.Fatalf("install = %d req=%+v body=%s", rec.Code, f.installReq, rec.Body)
	}

	rec = do(t, h, "POST", "/v1/plugins/unic.wenshushu/rollback", "")
	if rec.Code != 200 || f.rollbackID != "unic.wenshushu" {
		t.Fatalf("rollback = %d id=%q body=%s", rec.Code, f.rollbackID, rec.Body)
	}

	rec = do(t, h, "DELETE", "/v1/plugins/unic.wenshushu", "")
	if rec.Code != 200 || f.uninstallID != "unic.wenshushu" {
		t.Fatalf("uninstall = %d id=%q body=%s", rec.Code, f.uninstallID, rec.Body)
	}
}
