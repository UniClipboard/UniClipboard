package desktopui

import (
	"testing"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestPluginDisplayUsesLocalizedManifestText(t *testing.T) {
	item := api.PluginInfo{
		ID: "unic.wxime", ImplementationID: "unic.wxime", Version: "0.1.0",
		Name: "微信输入法", Summary: "默认简介",
		I18N: map[string]api.PluginText{"en": {Name: "WeChat IME"}, "ja": {Summary: "概要"}},
	}
	cases := []struct{ language, name, summary string }{
		{"", "微信输入法", "默认简介"},
		{"zh-CN", "微信输入法", "默认简介"},
		{"en", "WeChat IME", "默认简介"},
		{"ja", "微信输入法", "概要"},
		{"ru", "微信输入法", "默认简介"},
	}
	for _, c := range cases {
		state := config.State{Config: config.Default()}
		state.Config.Desktop.Language = c.language
		d := &viewState{live: &liveState{loaded: true, config: state}}
		if name, summary := d.pluginDisplay(item); name != c.name || summary != c.summary {
			t.Errorf("%q: got %q / %q", c.language, name, summary)
		}
	}
}

func TestPluginDisplayFallsBackWithoutRepeatingTheID(t *testing.T) {
	d := &viewState{live: &liveState{loaded: true, config: config.State{Config: config.Default()}}}
	item := api.PluginInfo{ID: "unic.wxime", ImplementationID: "unic.wxime", Version: "0.1.0"}
	name, summary := d.pluginDisplay(item)
	if name != "unic.wxime" || summary != "" {
		t.Fatalf("name=%q summary=%q", name, summary)
	}
	if got := pluginSubtitle(item, summary); got != "v0.1.0" {
		t.Fatalf("subtitle=%q", got)
	}
	item.ID = "unic.wxime.work"
	if got := pluginSubtitle(item, ""); got != "unic.wxime · v0.1.0" {
		t.Fatalf("instance subtitle=%q", got)
	}
}
