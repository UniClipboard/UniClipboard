package luascripts

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"
	"unicode"

	api "github.com/uniclipboard/unic/internal/localapi"
)

const (
	// DefaultMarketplaceURL is the static JSON plugin registry hosted on GitHub.
	// Override with UNIC_PLUGIN_REGISTRY_URL (or "off" for offline/bundled mode).
	DefaultMarketplaceURL = "https://raw.githubusercontent.com/UniClipboard/unic-plugins/main/registry.json"

	marketplaceCacheFile = "marketplace-cache.json"
	marketplaceCacheTTL  = 15 * time.Minute
	marketplaceMaxBytes  = 512 << 10
	marketplaceMaxItems  = 256
)

var validMarketplaceCategories = []string{"share", "text", "organize", "sync"}

type marketplaceCacheRecord struct {
	FetchedAt time.Time             `json:"fetched_at"`
	Catalog   api.PluginMarketplace `json:"catalog"`
}

// BundledMarketplace returns the built-in official catalog used when remote
// registry fetching is disabled or unreachable without a warm local cache.
func BundledMarketplace() api.PluginMarketplace {
	return api.PluginMarketplace{
		Version: 1,
		Source:  "bundled",
		Plugins: []api.MarketplaceEntry{
			{
				ID:            "unic.wenshushu",
				Name:          "文叔叔临时分享",
				Author:        "UniClipboard 官方",
				Official:      true,
				Category:      "share",
				Version:       "0.1.1",
				Summary:       "将选中的文本、图片或文件一键生成约 24 小时有效的临时下载链接。",
				Description:   "无需登录账号、无需任何配置。在剪贴板历史中选中文本、图片或文件并触发动作，插件会将所选内容上传至文叔叔，并将公开下载链接作为新历史条目返回。",
				Highlights:    []string{"免配置开箱即用", "支持文本 / 图片 / 文件（不含目录）", "约 24 小时自动过期"},
				HowTo:         []string{"在剪贴板历史中选中一条文本、图片或文件", "在右键菜单或工具栏打开「动作」并点击「通过文叔叔临时分享」", "上传完成后返回公开下载链接，原条目保持不变"},
				Privacy:       "仅在你主动点击分享动作时上传所选条目；安装或启用不会自动上传剪贴板内容。",
				ZeroConfig:    true,
				Repository:    "https://github.com/UniClipboard/unic-plugin-wenshushu",
				Ref:           "8e0c92fcb0a033cc0a934ef6ec193b391ad104e1",
				BuildFallback: true,
				Capabilities:  []string{"action"},
			},
			{
				ID:            "unic.wormhole",
				Name:          "Wormhole 临时分享",
				Author:        "UniClipboard 官方",
				Official:      true,
				Category:      "share",
				Version:       "0.1.0",
				Summary:       "将选中的文本、图片或文件生成端到端加密的临时下载链接。",
				Description:   "无需账号或配置。在剪贴板历史中选中条目并触发分享动作。上传完成后，链接会保存为一条新的历史记录。",
				Highlights:    []string{"端到端加密", "支持文本、图片和多个文件", "约 24 小时自动过期"},
				HowTo:         []string{"在剪贴板历史中选中一条文本、图片或文件", "打开「动作」并点击「通过 Wormhole 临时分享」", "将下载链接发送给接收方"},
				Privacy:       "仅在主动触发动作时上传加密内容。解密密钥在链接片段中，任何持有完整链接的人都可以下载并解密。",
				ZeroConfig:    true,
				Repository:    "https://github.com/UniClipboard/unic-plugin-wormhole",
				Ref:           "6b4fd2e7efae996539e1f00522003b312088b58d",
				BuildFallback: true,
				Capabilities:  []string{"action"},
			},
			{
				ID:            "unic.security",
				Name:          "敏感内容拦截",
				Author:        "UniClipboard 官方",
				Official:      true,
				Category:      "organize",
				Version:       "0.1.0",
				Summary:       "自动识别剪贴板中的密钥、密码管理器内容与敏感文件，拦截不入库且不同步。",
				Description:   "开箱即用的本地敏感内容保护插件。默认识别私钥、常见云/AI/GitHub 令牌与 JWT、含密码数据库连接串、密码/密钥键值对、1Password/Bitwarden/KeePassXC/Apple 密码等密码管理器来源应用以及 .env、id_rsa、*.pem/*.key/*.kdbx 等敏感文件，并阻止其写入本地历史或同步到其他设备；也可在插件设置中切换为仅本机保留/自动过期模式，或自定义应用名单与正则/关键词规则。",
				Highlights:    []string{"默认拦截不入库且不同步，免配置开箱即用", "支持私钥、API 密钥、密码管理器来源应用与敏感文件名", "支持自定义正则/关键词、白名单应用与仅本机过期模式"},
				HowTo:         []string{"在插件市场安装并启用「敏感内容拦截」，然后重启服务", "默认即刻拦截敏感内容不写入历史库且不跨设备同步", "按需在「设置 > 插件 > 敏感内容拦截」调整拦截模式、规则开关或自定义规则"},
				Privacy:       "100% 本地检测，零网络访问；绝不在日志、错误或通知中记录剪贴板原文、密钥值或文件名。",
				ZeroConfig:    true,
				Repository:    "https://github.com/UniClipboard/unic-plugin-security",
				Ref:           "0f23b17ee8922fdbe5e5e47016e0a70f979f1185",
				BuildFallback: true,
				Capabilities:  []string{"processor"},
			},
		},
	}
}

func marketplaceRegistryURL() string {
	if raw, ok := os.LookupEnv("UNIC_PLUGIN_REGISTRY_URL"); ok {
		return strings.TrimSpace(raw)
	}
	return DefaultMarketplaceURL
}

func marketplaceCachePath(root string) string {
	if root == "" {
		return ""
	}
	return filepath.Join(filepath.Dir(root), marketplaceCacheFile)
}

// FetchMarketplace loads the online plugin registry with disk cache and bundled
// fallback. Network or validation failures never fail the UI; they fall back to
// cache or bundled official plugins and attach a human-readable warning.
func FetchMarketplace(ctx context.Context, root string, refresh bool) api.PluginMarketplace {
	bundled := BundledMarketplace()
	regURL := marketplaceRegistryURL()
	if regURL == "" || strings.EqualFold(regURL, "off") || strings.EqualFold(regURL, "none") {
		return bundled
	}

	cachePath := marketplaceCachePath(root)
	var cached *marketplaceCacheRecord
	if cachePath != "" {
		if rec, err := readMarketplaceCache(cachePath); err == nil {
			cached = &rec
			if !refresh && time.Since(rec.FetchedAt) < marketplaceCacheTTL {
				out := rec.Catalog
				out.Source = "cache"
				out.Warning = ""
				return out
			}
		}
	}

	remote, err := fetchRemoteMarketplace(ctx, regURL)
	if err == nil {
		remote.Source = "remote"
		remote.Warning = ""
		if cachePath != "" {
			_ = writeMarketplaceCache(cachePath, marketplaceCacheRecord{FetchedAt: time.Now().UTC(), Catalog: remote})
		}
		return remote
	}

	if cached != nil {
		out := cached.Catalog
		out.Source = "cache"
		out.Warning = "未能连接在线插件市场，当前展示上次缓存目录。"
		return out
	}

	bundled.Warning = "未能连接在线插件市场，当前展示内置官方推荐目录。"
	return bundled
}

func fetchRemoteMarketplace(ctx context.Context, rawURL string) (api.PluginMarketplace, error) {
	u, err := url.Parse(rawURL)
	if err != nil || u.Host == "" {
		return api.PluginMarketplace{}, errors.New("invalid marketplace registry URL")
	}
	isLoopback := u.Hostname() == "127.0.0.1" || u.Hostname() == "localhost" || u.Hostname() == "::1"
	if u.Scheme != "https" && (u.Scheme != "http" || !isLoopback) {
		return api.PluginMarketplace{}, errors.New("marketplace registry URL must use https")
	}
	reqCtx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, u.String(), nil)
	if err != nil {
		return api.PluginMarketplace{}, err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "unic-desktop/plugin-marketplace")
	resp, err := (&http.Client{Timeout: 8 * time.Second}).Do(req)
	if err != nil {
		return api.PluginMarketplace{}, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return api.PluginMarketplace{}, errors.New("marketplace registry unavailable")
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, marketplaceMaxBytes+1))
	if err != nil || len(body) > marketplaceMaxBytes {
		return api.PluginMarketplace{}, errors.New("marketplace registry exceeds size limit")
	}
	return parseMarketplaceJSON(body)
}

func parseMarketplaceJSON(raw []byte) (api.PluginMarketplace, error) {
	var cat api.PluginMarketplace
	if err := json.Unmarshal(raw, &cat); err != nil {
		return api.PluginMarketplace{}, errors.New("invalid marketplace registry JSON")
	}
	if err := validateMarketplace(cat); err != nil {
		return api.PluginMarketplace{}, err
	}
	return cat, nil
}

func validateMarketplace(cat api.PluginMarketplace) error {
	if cat.Version != 1 || len(cat.Plugins) > marketplaceMaxItems {
		return errors.New("unsupported marketplace registry version or size")
	}
	seen := make(map[string]bool, len(cat.Plugins))
	for _, p := range cat.Plugins {
		if !ValidID(p.ID) || seen[p.ID] {
			return errors.New("invalid or duplicate marketplace plugin ID")
		}
		seen[p.ID] = true
		if !validText(p.Name, 1, 80) || !validText(p.Author, 1, 80) || !validText(p.Version, 1, 64) || !validText(p.Summary, 1, 240) {
			return errors.New("invalid marketplace plugin metadata")
		}
		if p.Description != "" && !validText(p.Description, 1, 2000) {
			return errors.New("invalid marketplace plugin description")
		}
		if p.Privacy != "" && !validText(p.Privacy, 1, 500) {
			return errors.New("invalid marketplace plugin privacy note")
		}
		if !slices.Contains(validMarketplaceCategories, p.Category) {
			return errors.New("invalid marketplace plugin category")
		}
		if _, err := openGitHub(p.Repository); err != nil {
			return errors.New("invalid marketplace plugin repository")
		}
		if p.Path != "" && !localPath(p.Path) {
			return errors.New("invalid marketplace plugin path")
		}
		if len(p.Highlights) > 8 || len(p.HowTo) > 8 {
			return errors.New("too many marketplace plugin details")
		}
		for _, h := range p.Highlights {
			if !validText(h, 1, 120) {
				return errors.New("invalid marketplace plugin highlight")
			}
		}
		for _, step := range p.HowTo {
			if !validText(step, 1, 240) {
				return errors.New("invalid marketplace plugin step")
			}
		}
	}
	return nil
}

func validText(s string, minLen, maxLen int) bool {
	trimmed := strings.TrimSpace(s)
	if len(trimmed) < minLen || len(trimmed) > maxLen {
		return false
	}
	return !strings.ContainsFunc(trimmed, func(r rune) bool {
		return unicode.IsControl(r) && r != '\n'
	})
}

func readMarketplaceCache(path string) (marketplaceCacheRecord, error) {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > marketplaceMaxBytes+4096 {
		return marketplaceCacheRecord{}, errors.New("marketplace cache unavailable")
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return marketplaceCacheRecord{}, err
	}
	var rec marketplaceCacheRecord
	if err := json.Unmarshal(raw, &rec); err != nil {
		return marketplaceCacheRecord{}, err
	}
	if err := validateMarketplace(rec.Catalog); err != nil {
		return marketplaceCacheRecord{}, err
	}
	return rec, nil
}

func writeMarketplaceCache(path string, rec marketplaceCacheRecord) error {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	raw, err := json.MarshalIndent(rec, "", "  ")
	if err != nil || len(raw) > marketplaceMaxBytes+4096 {
		return errors.New("marketplace cache too large")
	}
	return atomicWrite(filepath.Dir(path), filepath.Base(path), raw)
}
