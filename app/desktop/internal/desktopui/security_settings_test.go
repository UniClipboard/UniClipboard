package desktopui

import (
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/plugins/builtinsecurity"
	"github.com/uniclipboard/unic/plugin"
)

func TestBuiltinSecuritySettingsListDetailAndGuardStage(t *testing.T) {
	desk, cancel := newSmartPreviewTestDesktop(t)
	defer cancel()

	snapDir := os.Getenv("UNIC_BUILTIN_SECURITY_SNAP_DIR")
	saveSnap := func(name string, tt *ui.Tester) {
		if snapDir == "" {
			return
		}
		_ = os.MkdirAll(snapDir, 0o755)
		img := tt.Image()
		if img == nil {
			t.Fatalf("nil image for %s", name)
		}
		f, err := os.Create(filepath.Join(snapDir, name))
		if err != nil {
			t.Fatalf("create %s: %v", name, err)
		}
		defer func() { _ = f.Close() }()
		if err := png.Encode(f, img); err != nil {
			t.Fatalf("encode %s: %v", name, err)
		}
	}

	// 1. Built-in plugin list displays Sensitive Content Guard.
	ttList := ui.NewTesterWith(desk.View, 1100, 860, ui.TesterOptions{})
	if !ttList.HasText("敏感内容拦截") {
		t.Fatalf("expected builtin.security in built-in plugin list; texts=%v", ttList.Texts())
	}

	// 2. Detail page in default block mode hides mode-specific expire/tag rows cleanly.
	desk.d.pluginSettings.id = builtinsecurity.ID
	ttDefault := ui.NewTesterWith(desk.View, 1100, 1280, ui.TesterOptions{})
	for _, want := range []string{
		"敏感内容拦截", "启用敏感内容拦截", "处理失败时",
		"拦截策略", "拦截模式", "同时拦截远端接收内容", "拦截时显示通知",
		"内置检测规则", "私钥文本", "API 密钥、令牌与 JWT", "含密码的连接串",
		"密码与密钥键值对", "密码管理器来源应用", "敏感文件名", "最大扫描文本字节数",
		"应用与自定义规则", "额外拦截应用列表", "白名单应用列表", "自定义正则表达式", "自定义关键词",
	} {
		if !ttDefault.HasText(want) {
			t.Fatalf("expected %q on builtin.security detail page; texts=%v", want, ttDefault.Texts())
		}
	}
	if ttDefault.HasText("自动过期时长（秒）") || ttDefault.HasText("敏感标签") {
		t.Fatal("default block mode should hide expire_seconds and tag rows")
	}
	saveSnap("builtin-security-detail-light.png", ttDefault)

	// 2b. Hovering over a rule's '!' hint icon reveals concrete matching examples.
	if ttDefault.HasText("-----BEGIN OPENSSH PRIVATE KEY-----") {
		t.Fatal("rule example tooltip should not be visible before hover")
	}
	hintRect, ok := ttDefault.Find("查看示例：私钥文本")
	if !ok {
		t.Fatalf("missing '!' example hint icon for 私钥文本")
	}
	ttDefault.Move(hintRect.X+hintRect.W/2, hintRect.Y+hintRect.H/2)
	ttDefault.Advance(2 * time.Second)
	for _, want := range []string{"匹配示例", "-----BEGIN OPENSSH PRIVATE KEY-----", "-----BEGIN PGP PRIVATE KEY BLOCK-----"} {
		if !ttDefault.HasText(want) {
			t.Fatalf("expected %q in hovered example tooltip; texts=%v", want, ttDefault.Texts())
		}
	}
	saveSnap("builtin-security-tooltip-light.png", ttDefault)

	// Verify API token rule hint icon also shows examples on hover.
	tokRect, ok := ttDefault.Find("查看示例：API 密钥、令牌与 JWT")
	if !ok {
		t.Fatalf("missing '!' example hint icon for API 密钥、令牌与 JWT")
	}
	ttDefault.Move(tokRect.X+tokRect.W/2, tokRect.Y+tokRect.H/2)
	ttDefault.Advance(2 * time.Second)
	if !ttDefault.HasText("AKIAIOSFODNN7EXAMPLE") {
		t.Fatalf("expected AWS example in hovered token tooltip; texts=%v", ttDefault.Texts())
	}

	// 3. Expire mode reveals both expire_seconds and tag rows.
	desk.d.live.config.Revision = "rev-2"
	desk.d.live.config.Config.Plugins.Instances = map[string]config.Instance{
		builtinsecurity.ID: {
			Security: &builtinsecurity.Settings{
				Mode: builtinsecurity.ModeExpire,
			},
		},
	}
	ttExpire := ui.NewTesterWith(desk.View, 1100, 1360, ui.TesterOptions{})
	if !ttExpire.HasText("自动过期时长（秒）") || !ttExpire.HasText("敏感标签") {
		t.Fatalf("expire mode should show expire_seconds and tag rows; texts=%v", ttExpire.Texts())
	}

	// 4. Advanced view places builtin.security in Guard stage ahead of Annotate and Sync even with third-party sync installed.
	desk.d.live.externalPlugins = []api.PluginInfo{{
		ID:               "thirdparty.sync",
		ImplementationID: "thirdparty.sync",
		Name:             "第三方同步插件",
		Version:          "1.0.0",
		Kind:             "go",
		Capabilities:     []string{plugin.CapSyncBackend},
		TransportKind:    plugin.TransportObjectStore,
		ExternalChannels: true,
		Configured:       true,
	}}
	desk.d.live.config.Config.Plugins.Order = []string{"thirdparty.sync"}
	desk.d.pluginSettings.id = ""
	desk.d.pluginSettings.view = "advanced"
	ttAdv := ui.NewTesterWith(desk.View, 1100, 960, ui.TesterOptions{})
	for _, want := range []string{"前置拦截顺序（Guard）", "1. 敏感内容拦截", "builtin.security", "内容处理顺序（Annotate）", "同步后端（本地保存后执行）", "第三方同步插件"} {
		if !ttAdv.HasText(want) {
			t.Fatalf("expected %q in advanced view with builtin.security; texts=%v", want, ttAdv.Texts())
		}
	}
}
