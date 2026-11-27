package desktopui

import (
	"encoding/json"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

// Real settings.schema.json from UniClipboard/unic-plugin-security.
const unicSecuritySchemaJSON = `{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "x-unic-settings-version": 1,
  "type": "object",
  "properties": {
    "mode": {
      "type": "string",
      "title": "Interception mode",
      "description": "Action taken when sensitive content is detected.",
      "enum": ["block", "block_sync", "expire"],
      "default": "block",
      "x-unic-ui": {
        "widget": "select",
        "order": 1,
        "group": "Interception",
        "option_labels": [
          "Block (do not save or sync)",
          "Keep locally only (no sync)",
          "Keep locally with auto-expiry"
        ],
        "i18n": {
          "zh-CN": {
            "title": "拦截模式",
            "description": "识别到敏感内容时的处理方式。",
            "group": "拦截策略",
            "option_labels": [
              "拦截不入库且不同步",
              "仅本机保留（禁止同步）",
              "本机短暂保留后自动过期（禁止同步）"
            ]
          }
        }
      }
    },
    "expire_seconds": {
      "type": "integer",
      "title": "Expiry duration (seconds)",
      "description": "Used when mode is auto-expiry. Sensitive items expire from local history after this duration.",
      "default": 300,
      "minimum": 10,
      "maximum": 86400,
      "x-unic-ui": {
        "order": 2,
        "group": "Interception",
        "i18n": {
          "zh-CN": {
            "title": "自动过期时长（秒）",
            "description": "仅在「本机短暂保留后自动过期」模式下生效。",
            "group": "拦截策略"
          }
        }
      }
    },
    "tag": {
      "type": "string",
      "title": "Sensitive tag",
      "description": "Tag applied when keeping sensitive items locally (block_sync or expire mode).",
      "default": "sensitive",
      "minLength": 1,
      "maxLength": 64,
      "x-unic-ui": {
        "order": 3,
        "group": "Interception",
        "i18n": {
          "zh-CN": {
            "title": "敏感标签",
            "description": "在本地保留模式下为敏感条目添加的标签。",
            "group": "拦截策略"
          }
        }
      }
    },
    "intercept_received": {
      "type": "boolean",
      "title": "Filter received items",
      "description": "Also intercept sensitive content received from other devices or external channels.",
      "default": true,
      "x-unic-ui": {
        "order": 4,
        "group": "Interception",
        "i18n": {
          "zh-CN": {
            "title": "同时拦截远端接收内容",
            "description": "对从其他设备或外部渠道同步接收的敏感内容同样执行拦截。",
            "group": "拦截策略"
          }
        }
      }
    },
    "notify": {
      "type": "boolean",
      "title": "Show notification",
      "description": "Show a short notification naming only the rule category when an item is intercepted. Clipboard text is never included.",
      "default": true,
      "x-unic-ui": {
        "order": 5,
        "group": "Interception",
        "i18n": {
          "zh-CN": {
            "title": "拦截时显示通知",
            "description": "拦截时展示仅包含规则类别的简短提示，绝不包含剪贴板原文。",
            "group": "拦截策略"
          }
        }
      }
    },
    "detect_private_keys": {
      "type": "boolean",
      "title": "Private keys",
      "description": "Detect PEM/OpenSSH/PGP private key headers.",
      "default": true,
      "x-unic-ui": {
        "order": 10,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "私钥与证书密钥",
            "description": "识别 PEM、OpenSSH、PGP 私钥块。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "detect_tokens_and_jwt": {
      "type": "boolean",
      "title": "API tokens & JWTs",
      "description": "Detect common cloud/AI API tokens, Authorization headers, and JWTs.",
      "default": true,
      "x-unic-ui": {
        "order": 11,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "API 密钥、令牌与 JWT",
            "description": "识别常见云服务与 AI 平台的 API 密钥、Authorization 凭据头以及 JWT。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "detect_connection_strings": {
      "type": "boolean",
      "title": "Database connection strings",
      "description": "Detect database and service URIs containing embedded passwords.",
      "default": true,
      "x-unic-ui": {
        "order": 12,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "含密码的连接串",
            "description": "识别包含明文密码的数据库或服务连接 URI。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "detect_credentials": {
      "type": "boolean",
      "title": "Password & secret assignments",
      "description": "Detect password, secret, and api_key key-value assignments while ignoring common placeholders.",
      "default": true,
      "x-unic-ui": {
        "order": 13,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "密码与密钥键值对",
            "description": "识别 .env、YAML、JSON 等格式中的 password、secret、api_key 赋值（自动忽略常见占位符）。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "detect_password_managers": {
      "type": "boolean",
      "title": "Password manager apps",
      "description": "Automatically intercept content copied from known password managers.",
      "default": true,
      "x-unic-ui": {
        "order": 14,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "密码管理器来源应用",
            "description": "自动拦截来自 1Password、Bitwarden、KeePassXC、Apple 密码/钥匙串等密码管理器的剪贴板内容。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "detect_sensitive_files": {
      "type": "boolean",
      "title": "Sensitive filenames",
      "description": "Intercept copied files matching .env, id_rsa, *.pem, *.key, *.kdbx, etc.",
      "default": true,
      "x-unic-ui": {
        "order": 15,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "敏感文件名",
            "description": "拦截 .env、id_rsa、*.pem、*.key、*.kdbx 等敏感文件。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "max_scan_bytes": {
      "type": "integer",
      "title": "Maximum text scan size (bytes)",
      "description": "Upper bound of text bytes scanned per clipboard item.",
      "default": 65536,
      "minimum": 1024,
      "maximum": 1048576,
      "x-unic-ui": {
        "order": 16,
        "group": "Built-in Rules",
        "i18n": {
          "zh-CN": {
            "title": "最大扫描文本字节数",
            "description": "单条文本内容参与敏感检测的最大前缀字节数。",
            "group": "内置检测规则"
          }
        }
      }
    },
    "blocked_apps": {
      "type": "array",
      "title": "Additional blocked apps",
      "description": "Always intercept items copied from these source application identifiers or display names.",
      "items": {"type": "string"},
      "default": [],
      "maxItems": 64,
      "x-unic-ui": {
        "order": 20,
        "group": "Apps & Custom Rules",
        "i18n": {
          "zh-CN": {
            "title": "额外拦截应用列表",
            "description": "填写应用标识符（Bundle ID / 进程名）或应用名称，来自这些应用的内容将一律被拦截。",
            "group": "应用与自定义规则"
          }
        }
      }
    },
    "allowed_apps": {
      "type": "array",
      "title": "Allowed apps (bypass)",
      "description": "Skip all sensitivity checks for items copied from these source application identifiers or display names.",
      "items": {"type": "string"},
      "default": [],
      "maxItems": 64,
      "x-unic-ui": {
        "order": 21,
        "group": "Apps & Custom Rules",
        "i18n": {
          "zh-CN": {
            "title": "白名单应用列表",
            "description": "来自这些应用的内容将跳过所有敏感检测规则。",
            "group": "应用与自定义规则"
          }
        }
      }
    },
    "custom_patterns": {
      "type": "array",
      "title": "Custom regular expressions",
      "description": "Additional Go RE2 regular expressions that mark text as sensitive.",
      "items": {"type": "string"},
      "default": [],
      "maxItems": 32,
      "x-unic-ui": {
        "order": 22,
        "group": "Apps & Custom Rules",
        "i18n": {
          "zh-CN": {
            "title": "自定义正则表达式",
            "description": "使用 Go RE2 语法添加额外的敏感文本匹配规则。",
            "group": "应用与自定义规则"
          }
        }
      }
    },
    "custom_keywords": {
      "type": "array",
      "title": "Custom keywords",
      "description": "Case-insensitive literal substrings that mark text as sensitive.",
      "items": {"type": "string"},
      "default": [],
      "maxItems": 64,
      "x-unic-ui": {
        "order": 23,
        "group": "Apps & Custom Rules",
        "i18n": {
          "zh-CN": {
            "title": "自定义关键词",
            "description": "不区分大小写的字面量关键词，命中即视为敏感内容。",
            "group": "应用与自定义规则"
          }
        }
      }
    }
  },
  "required": [
    "mode", "expire_seconds", "tag", "intercept_received", "notify",
    "detect_private_keys", "detect_tokens_and_jwt", "detect_connection_strings",
    "detect_credentials", "detect_password_managers", "detect_sensitive_files",
    "max_scan_bytes", "blocked_apps", "allowed_apps", "custom_patterns", "custom_keywords"
  ],
  "additionalProperties": false
}`

const unicSecurityDefaultValuesJSON = `{
  "mode": "block",
  "expire_seconds": 300,
  "tag": "sensitive",
  "intercept_received": true,
  "notify": true,
  "detect_private_keys": true,
  "detect_tokens_and_jwt": true,
  "detect_connection_strings": true,
  "detect_credentials": true,
  "detect_password_managers": true,
  "detect_sensitive_files": true,
  "max_scan_bytes": 65536,
  "blocked_apps": ["com.agilebits.onepassword7"],
  "allowed_apps": [],
  "custom_patterns": [],
  "custom_keywords": []
}`

func TestUnicSecuritySchemaFormAndStagedOrderUI(t *testing.T) {
	desk, cancel := newSmartPreviewTestDesktop(t)
	defer cancel()

	secPlugin := api.PluginInfo{
		ID:               "unic.security",
		ImplementationID: "unic.security",
		Name:             "Sensitive Content Guard",
		Icon:             "shield",
		Summary:          "Detect sensitive clipboard content and keep it out of history and sync.",
		I18N: map[string]api.PluginText{
			"zh-CN": {
				Name:    "敏感内容拦截",
				Summary: "自动识别剪贴板中的密钥、密码管理器内容与敏感文件，拦截不入库且不同步。",
			},
		},
		Version:        "0.1.1",
		Kind:           "go",
		Source:         "prebuilt",
		Capabilities:   []string{plugin.CapProcessor},
		ProcessorStage: plugin.ProcessorStageGuard,
		Configured:     true,
		RunningVersion: "0.1.1",
		HasSettings:    true,
	}
	thirdPartySync := api.PluginInfo{
		ID:               "thirdparty.sync",
		ImplementationID: "thirdparty.sync",
		Name:             "第三方同步插件",
		Version:          "1.0.0",
		Kind:             "go",
		Source:           "prebuilt",
		Capabilities:     []string{plugin.CapSyncBackend},
		TransportKind:    plugin.TransportObjectStore,
		ExternalChannels: true,
		Configured:       true,
		RunningVersion:   "1.0.0",
	}
	thirdPartyAnnotator := api.PluginInfo{
		ID:               "thirdparty.tagger",
		ImplementationID: "thirdparty.tagger",
		Name:             "第三方标注插件",
		Version:          "1.0.0",
		Kind:             "lua",
		Source:           "local",
		Capabilities:     []string{plugin.CapProcessor},
		Configured:       true,
		RunningVersion:   "1.0.0",
	}

	desk.d.live.externalPlugins = []api.PluginInfo{thirdPartySync, thirdPartyAnnotator, secPlugin}
	// Simulate user installing third-party sync & annotator first, then unic.security last.
	desk.d.live.config.Config.Plugins.Order = []string{"thirdparty.sync", "thirdparty.tagger", "unic.security"}
	desk.d.live.config.Config.Desktop.Language = "zh-CN"

	form := &pluginSettingsForm{}
	if err := form.load(api.PluginSettings{
		Revision: "rev-sec-1",
		Schema:   json.RawMessage(unicSecuritySchemaJSON),
		Values:   json.RawMessage(unicSecurityDefaultValuesJSON),
		Secrets:  map[string]bool{},
	}); err != nil {
		t.Fatalf("load unic.security schema: %v", err)
	}

	snapDir := os.Getenv("UNIC_PLUGIN_UI_SNAP_DIR")
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

	// 1. Render unic.security detail form in zh-CN.
	desk.d.pluginSettings.id = "unic.security"
	desk.d.pluginSettings.form = form
	tt := ui.NewTesterWith(desk.View, 1100, 1400, ui.TesterOptions{})
	for _, want := range []string{
		"敏感内容拦截", "前置拦截", "处理失败时",
		"拦截策略", "拦截模式", "自动过期时长（秒）", "敏感标签", "同时拦截远端接收内容", "拦截时显示通知",
		"内置检测规则", "私钥与证书密钥", "API 密钥、令牌与 JWT", "最大扫描文本字节数",
		"应用与自定义规则", "额外拦截应用列表", "白名单应用列表", "暂无配置项",
	} {
		if !tt.HasText(want) {
			t.Fatalf("expected %q on unic.security settings form; texts=%v", want, tt.Texts())
		}
	}
	saveSnap("unic-security-form-zh-light.png", tt)

	// 2. Verify invalid numeric value surfaces a clear range error.
	form.values["expire_seconds"] = json.Number("5")
	ttInvalid := ui.NewTesterWith(desk.View, 1100, 1400, ui.TesterOptions{})
	if !ttInvalid.HasText("请输入 10 到 86400 之间的数值。") || !ttInvalid.HasText("请修正无效配置项后再保存。") {
		t.Fatalf("expected range error for expire_seconds=5; texts=%v", ttInvalid.Texts())
	}
	form.values["expire_seconds"] = json.Number("300")

	// 3. Render staged execution order view: guard appears in its own stage ahead of annotate & sync.
	desk.d.pluginSettings.id = ""
	desk.d.pluginSettings.form = nil
	desk.d.pluginSettings.view = "advanced"
	ttAdv := ui.NewTesterWith(desk.View, 1100, 960, ui.TesterOptions{})
	for _, want := range []string{
		"前置拦截顺序（Guard）",
		"内容处理顺序（Annotate）",
		"异步富化与预览优先级（Enricher）",
		"同步后端（本地保存后执行）",
		"1. 敏感内容拦截",
		"1. 内容分类",
		"2. 文本工具",
		"3. 第三方标注插件",
		"第三方同步插件",
	} {
		if !ttAdv.HasText(want) {
			t.Fatalf("expected %q in staged advanced view; texts=%v", want, ttAdv.Texts())
		}
	}
	saveSnap("plugins-advanced-staged-zh-light.png", ttAdv)
}

func TestReorderPluginStagePreservesSyncBackendsAndOtherStages(t *testing.T) {
	configured := []string{"thirdparty.sync", "thirdparty.tagger", "unic.security"}
	annotateStage := []string{"builtin.classify", "builtin.text", "thirdparty.tagger"}

	next := reorderPluginStage(configured, annotateStage, 2, -1)
	// Sync backend and guard plugin must remain enabled in plugins.order.
	for _, mustKeep := range []string{"thirdparty.sync", "unic.security", "thirdparty.tagger"} {
		if !slices.Contains(next, mustKeep) {
			t.Fatalf("reorderPluginStage dropped %q: got %v", mustKeep, next)
		}
	}
	// Within annotateStage, thirdparty.tagger moved ahead of builtin.text.
	eff := stageEffectiveOrder(next, annotateStage)
	want := []string{"builtin.classify", "thirdparty.tagger", "builtin.text"}
	if !slices.Equal(eff, want) {
		t.Fatalf("effective annotate order = %v, want %v", eff, want)
	}
}
