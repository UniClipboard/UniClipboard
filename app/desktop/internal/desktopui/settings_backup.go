package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"sort"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// MaxSettingsBackupBytes bounds native backup reads and parser input.
const MaxSettingsBackupBytes = 256 << 10
const settingsBackupFormat = "unic-portable-preferences"

// The explicit field list is intentionally independent of Config's full shape.
// Adding a private config field cannot silently expand the backup surface.
type portableSettings struct {
	Language          string      `json:"desktop.language"`
	ColorMode         string      `json:"desktop.color_mode"`
	TrayIcon          string      `json:"desktop.tray_icon"`
	UpdateChannel     string      `json:"desktop.update_channel"`
	SystemWindowFrame bool        `json:"desktop.system_window_frame"`
	Mode              string      `json:"desktop.mode"`
	StartInBackground bool        `json:"desktop.start_in_background"`
	LoggingLevel      string      `json:"logging.level"`
	TelemetryEnabled  bool        `json:"telemetry.enabled"`
	LogsEnabled       bool        `json:"telemetry.logs.enabled"`
	LogsLevel         string      `json:"telemetry.logs.level"`
	MetricsEnabled    bool        `json:"telemetry.metrics.enabled"`
	TracesEnabled     bool        `json:"telemetry.traces.enabled"`
	AnalyticsEnabled  bool        `json:"telemetry.analytics.enabled"`
	SyncEnabled       bool        `json:"sync.enabled"`
	SyncAutomatic     bool        `json:"sync.automatic"`
	SyncOnRestore     bool        `json:"sync.on_restore"`
	SyncFiles         bool        `json:"sync.files"`
	SyncDirectories   bool        `json:"sync.directories.automatic"`
	HistoryCleanup    bool        `json:"history.auto_cleanup"`
	RetentionDays     int         `json:"history.retention_days"`
	MaxItems          int         `json:"history.max_items"`
	HistoryBytes      config.Size `json:"history.max_bytes"`
	CacheBytes        config.Size `json:"file_cache.max_bytes"`
	MaxFileBytes      config.Size `json:"limits.max_file_bytes"`
	MaxTotalBytes     config.Size `json:"limits.max_total_bytes"`
}

type settingsBackupDocument struct {
	Format      string          `json:"format"`
	Version     int             `json:"version"`
	Preferences json.RawMessage `json:"preferences"`
}

type backupChange struct{ key, before, after string }
type backupPreview struct {
	config   config.Config
	revision string
	changes  []backupChange
}
type backupSettingsState struct {
	busy, confirmOpen, scopeOpen bool
	preview                      *backupPreview
	err, notice                  string
	export                       func(context.Context, []byte) (string, error)
	read                         func(context.Context) ([]byte, error)
}

func portableSettingsFrom(c config.Config) portableSettings {
	return portableSettings{
		Language: c.Desktop.Language, ColorMode: c.Desktop.ColorMode, TrayIcon: c.Desktop.TrayIcon, UpdateChannel: c.Desktop.UpdateChannel,
		SystemWindowFrame: c.Desktop.SystemWindowFrame, Mode: c.Desktop.Mode, StartInBackground: c.Desktop.StartInBackground,
		LoggingLevel: c.Logging.Level, TelemetryEnabled: c.Telemetry.Enabled, LogsEnabled: c.Telemetry.Logs.Enabled, LogsLevel: c.Telemetry.Logs.Level,
		MetricsEnabled: c.Telemetry.Metrics.Enabled, TracesEnabled: c.Telemetry.Traces.Enabled, AnalyticsEnabled: c.Telemetry.Analytics.Enabled,
		SyncEnabled: c.Sync.Enabled, SyncAutomatic: c.Sync.Automatic, SyncOnRestore: c.Sync.OnRestore, SyncFiles: c.Sync.Files, SyncDirectories: c.Sync.Directories.Automatic,
		HistoryCleanup: c.History.AutoCleanup, RetentionDays: c.History.RetentionDays, MaxItems: c.History.MaxItems, HistoryBytes: c.History.MaxBytes,
		CacheBytes: c.FileCache.MaxBytes, MaxFileBytes: c.Limits.MaxFileBytes, MaxTotalBytes: c.Limits.MaxTotalBytes,
	}
}

func (p portableSettings) apply(c config.Config) config.Config {
	c.Desktop.Language = p.Language
	c.Desktop.ColorMode = p.ColorMode
	c.Desktop.TrayIcon = p.TrayIcon
	c.Desktop.UpdateChannel = p.UpdateChannel
	c.Desktop.SystemWindowFrame = p.SystemWindowFrame
	c.Desktop.Mode = p.Mode
	c.Desktop.StartInBackground = p.StartInBackground
	c.Logging.Level = p.LoggingLevel
	c.Telemetry.Enabled = p.TelemetryEnabled
	c.Telemetry.Logs.Enabled = p.LogsEnabled
	c.Telemetry.Logs.Level = p.LogsLevel
	c.Telemetry.Metrics.Enabled = p.MetricsEnabled
	c.Telemetry.Traces.Enabled = p.TracesEnabled
	c.Telemetry.Analytics.Enabled = p.AnalyticsEnabled
	c.Sync.Enabled = p.SyncEnabled
	c.Sync.Automatic = p.SyncAutomatic
	c.Sync.OnRestore = p.SyncOnRestore
	c.Sync.Files = p.SyncFiles
	c.Sync.Directories.Automatic = p.SyncDirectories
	c.History.AutoCleanup = p.HistoryCleanup
	c.History.RetentionDays = p.RetentionDays
	c.History.MaxItems = p.MaxItems
	c.History.MaxBytes = p.HistoryBytes
	c.FileCache.MaxBytes = p.CacheBytes
	c.Limits.MaxFileBytes = p.MaxFileBytes
	c.Limits.MaxTotalBytes = p.MaxTotalBytes
	return c
}

func encodePortableSettings(c config.Config) ([]byte, error) {
	if err := c.Validate(); err != nil {
		return nil, errors.New("settings are invalid")
	}
	preferences, err := json.Marshal(portableSettingsFrom(c))
	if err != nil {
		return nil, errors.New("cannot encode portable settings")
	}
	data, err := json.MarshalIndent(settingsBackupDocument{Format: settingsBackupFormat, Version: 1, Preferences: preferences}, "", "  ")
	if err != nil || len(data) >= MaxSettingsBackupBytes {
		return nil, errors.New("cannot encode portable settings")
	}
	return append(data, '\n'), nil
}

func decodeBackupJSON(data []byte, value any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return errors.New("invalid settings backup")
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return errors.New("settings backup has trailing content")
	}
	return nil
}

func previewPortableSettings(data []byte, current config.State) (*backupPreview, error) {
	if len(data) == 0 || len(data) > MaxSettingsBackupBytes || current.Revision == "" || current.Error != "" {
		return nil, errors.New("settings backup or current configuration is unavailable")
	}
	var doc settingsBackupDocument
	if err := decodeBackupJSON(data, &doc); err != nil {
		return nil, err
	}
	if doc.Format != settingsBackupFormat || doc.Version != 1 {
		return nil, errors.New("unsupported settings backup")
	}
	var prefs portableSettings
	if err := decodeBackupJSON(doc.Preferences, &prefs); err != nil {
		return nil, err
	}
	// Require every portable field. Missing values must never silently reset a
	// preference to Go's zero value. Unknown fields were rejected above.
	var incoming map[string]json.RawMessage
	if err := json.Unmarshal(doc.Preferences, &incoming); err != nil {
		return nil, errors.New("invalid settings backup")
	}
	defaults, _ := json.Marshal(portableSettings{})
	var expected map[string]json.RawMessage
	if err := json.Unmarshal(defaults, &expected); err != nil {
		return nil, errors.New("cannot inspect portable settings")
	}
	if len(incoming) != len(expected) {
		return nil, errors.New("settings backup is incomplete")
	}
	for key := range expected {
		if value, ok := incoming[key]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return nil, errors.New("settings backup is incomplete")
		}
	}
	merged := prefs.apply(current.Config)
	if err := merged.Validate(); err != nil {
		return nil, errors.New("settings backup values are invalid")
	}
	previous, _ := json.Marshal(portableSettingsFrom(current.Config))
	var old map[string]json.RawMessage
	if err := json.Unmarshal(previous, &old); err != nil {
		return nil, errors.New("cannot inspect current settings")
	}
	result := &backupPreview{config: merged, revision: current.Revision}
	// Canonical typed serialization makes textual whitespace irrelevant.
	currentValues, err := json.Marshal(prefs)
	if err != nil {
		return nil, errors.New("cannot inspect portable settings")
	}
	var canonical map[string]json.RawMessage
	if err := json.Unmarshal(currentValues, &canonical); err != nil {
		return nil, errors.New("cannot inspect portable settings")
	}
	for key, value := range canonical {
		if !bytes.Equal(old[key], value) {
			result.changes = append(result.changes, backupChange{key: key, before: string(old[key]), after: string(value)})
		}
	}
	sort.Slice(result.changes, func(i, j int) bool { return result.changes[i].key < result.changes[j].key })
	return result, nil
}

// SetBackupActions binds native file panels. Callbacks run outside the shared
// service job queue. Nil data/path denotes cancellation rather than success.
func (s *Desktop) SetBackupActions(export func(context.Context, []byte) (string, error), read func(context.Context) ([]byte, error)) {
	s.d.backup.export = export
	s.d.backup.read = read
}

func (d *viewState) prepareBackupImport(data []byte, current config.State) {
	s := &d.backup
	s.preview = nil
	s.confirmOpen = false
	s.err = ""
	s.notice = ""
	preview, err := previewPortableSettings(data, current)
	if err != nil {
		s.err = d.tr("无法导入此配置备份。请检查格式、版本和设置值。")
		return
	}
	s.preview = preview
	if len(preview.changes) == 0 {
		s.notice = d.tr("备份与当前可移动设置相同。")
	}
}

func (d *viewState) cancelBackupImport() { d.backup.preview = nil; d.backup.confirmOpen = false }

func (d *viewState) confirmBackupImport() {
	l := d.live
	s := &d.backup
	if l == nil || l.busy || s.busy || s.preview == nil {
		return
	}
	preview := s.preview
	if l.config.Revision != preview.revision {
		s.err = d.tr("设置已更改。请重新选择备份并检查预览。")
		s.confirmOpen = false
		return
	}
	if len(preview.changes) == 0 {
		d.cancelBackupImport()
		return
	}
	if err := preview.config.Validate(); err != nil {
		s.err = d.tr("无法导入此配置备份。请检查格式、版本和设置值。")
		return
	}
	s.preview = nil
	s.confirmOpen = false
	s.err = ""
	s.notice = ""
	d.saveLiveConfigThen(preview.config, func() { s.notice = d.tr("配置已导入。请检查需要重启的设置。") })
}

func (d *viewState) runBackupAction(export bool) {
	s := &d.backup
	l := d.live
	if l == nil || !l.loaded || l.busy || s.busy || (export && s.export == nil) || (!export && s.read == nil) {
		return
	}
	current := l.config
	if current.Error != "" {
		s.err = d.tr("当前配置无效。请先修复配置。")
		return
	}
	data, err := encodePortableSettings(current.Config)
	if err != nil {
		s.err = d.tr("无法导出当前设置。")
		return
	}
	s.busy = true
	s.err = ""
	s.notice = ""
	s.preview = nil
	s.confirmOpen = false
	exportAction, readAction := s.export, s.read
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Minute)
		defer cancel()
		var output string
		var loaded []byte
		var err error
		if export {
			output, err = exportAction(ctx, data)
		} else {
			loaded, err = readAction(ctx)
		}
		if ctx.Err() != nil {
			err = ctx.Err()
		}
		apply := func() {
			s.busy = false
			if err != nil {
				s.err = d.tr("配置文件操作失败。请重试。")
				return
			}
			if export {
				if output == "" {
					s.notice = d.tr("已取消导出。")
				} else {
					s.notice = d.tr("配置已导出：") + output
				}
			} else if len(loaded) == 0 {
				s.notice = d.tr("已取消导入。")
			} else {
				d.prepareBackupImport(loaded, current)
			}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

// Preview labels describe the user preference rather than exposing config keys.
var portableSettingLabels = map[string]string{
	"desktop.language": "界面语言", "desktop.color_mode": "颜色模式", "desktop.tray_icon": "托盘图标", "desktop.update_channel": "更新渠道",
	"desktop.system_window_frame": "使用系统边框", "desktop.mode": "运行模式", "desktop.start_in_background": "后台启动",
	"logging.level": "本机日志等级", "telemetry.enabled": "发送使用数据与诊断", "telemetry.logs.enabled": "上传诊断日志", "telemetry.logs.level": "上传日志等级",
	"telemetry.metrics.enabled": "上传性能指标", "telemetry.traces.enabled": "上传采样追踪", "telemetry.analytics.enabled": "上传匿名使用事件",
	"sync.enabled": "启用同步", "sync.automatic": "自动同步", "sync.on_restore": "使用历史时同步", "sync.files": "同步文件", "sync.directories.automatic": "自动同步目录",
	"history.auto_cleanup": "自动清理历史", "history.retention_days": "历史保留时间", "history.max_items": "最大历史条目数", "history.max_bytes": "历史存储预算",
	"file_cache.max_bytes": "单设备缓存配额", "limits.max_file_bytes": "单文件传输上限", "limits.max_total_bytes": "内容总大小上限",
}

func (d *viewState) backupDisplayValue(key, value string) string {
	switch value {
	case "true":
		return d.tr("开启")
	case "false":
		return d.tr("关闭")
	}
	switch key {
	case "history.max_bytes", "file_cache.max_bytes", "limits.max_file_bytes", "limits.max_total_bytes":
		if size, err := strconv.ParseInt(value, 10, 64); err == nil {
			return humanBytes(size)
		}
	case "history.retention_days":
		if value == "0" {
			return d.tr("无限制")
		}
		return value + " " + d.tr("天")
	case "history.max_items":
		if value == "0" {
			return d.tr("无限制")
		}
		return value + " " + d.tr("条")
	}
	var text string
	if json.Unmarshal([]byte(value), &text) == nil {
		switch text {
		case "system":
			return d.tr("跟随系统")
		case "light":
			return d.tr("浅色")
		case "dark":
			return d.tr("深色")
		case "glyph":
			return d.tr("默认")
		case "cat":
			return d.tr("经典猫头")
		case "normal":
			return d.tr("常规模式")
		case "lightweight":
			return d.tr("轻量模式")
		case "stable":
			return d.tr("稳定版")
		case "alpha":
			return d.tr("内测版")
		}
		return text
	}
	return value
}

func (d *viewState) backupSettingsContent(c *ui.Context, k desktopkit.Kit) {
	s := &d.backup
	l := d.live
	disabled := s.busy || l.busy || !l.loaded || l.config.Error != ""
	k.SettingsGroup(c.Key("configuration-backup"), desktopkit.SettingsGroupProps{
		Title: d.tr("配置备份"),
		Trailing: func() {
			settingsPill(c.Key("scope-pill"), k, d.tr("仅 26 项可移动偏好"), desktopkit.StatusNeutral, false, false)
		},
	}, func() {
		k.SettingsRow(c.Key("export-row"), desktopkit.SettingsRowProps{
			Label:       d.tr("导出配置"),
			Description: d.tr("生成可跨设备迁移的 JSON 偏好文件（绝不覆盖已有目标文件）。"),
		}, nil, func() {
			if k.Button(c.Key("export-settings"), d.tr("导出配置"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconDownload,
				Disabled: disabled || s.export == nil,
			}).Clicked() {
				d.runBackupAction(true)
			}
		})
		k.SettingsRow(c.Key("import-row"), desktopkit.SettingsRowProps{
			Label:       d.tr("导入配置"),
			Description: d.tr("读取后严格校验并展示逐项对比预览，二次确认后按原修订版本写入。"),
		}, nil, func() {
			if k.Button(c.Key("import-settings"), d.tr("导入配置"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconUpdate,
				Disabled: disabled || s.read == nil,
			}).Clicked() {
				d.runBackupAction(false)
			}
		})
		if s.err != "" {
			settingsStrip(c.Key("err"), k, desktopkit.ToneWarning, s.err)
		}
		if s.notice != "" {
			settingsStrip(c.Key("notice"), k, desktopkit.ToneInfo, s.notice)
		}

		// Collapsible scope disclosure row matching prototype/settings.html.
		toggle := ui.ButtonBase(c.Key("scope-toggle")).
			Label(d.tr("查看备份包含与排除范围")).
			Expanded(s.scopeOpen).
			FillWidth().Padding(9, 16).
			Background(k.SubtleCardSurface()).
			AlignItems(ui.Center).Justify(ui.SpaceBetween)
		toggle.Children(func() {
			k.Text(c.Key("l"), d.tr("查看备份包含与排除范围"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
			ic := desktopkit.IconChevronRight
			if s.scopeOpen {
				ic = desktopkit.IconChevronDown
			}
			k.Icon(c.Key("chev"), ic, 14).TextColor(k.Color("text-secondary"))
		})
		if toggle.Clicked() {
			s.scopeOpen = !s.scopeOpen
		}
		if s.scopeOpen {
			ui.Row(c.Key("scope-cols")).Wrap().Padding(12, 16, 14).Gap(14).Background(k.SubtleCardSurface()).MinWidth(0).Children(func() {
				ui.Column(c.Key("inc")).Basis(240).Grow(1).MinWidth(0).Gap(5).Children(func() {
					ui.Text(c.Key("h"), d.tr("✓ 包含（26 项白名单偏好）")).FontSize(12).FontWeight(600).TextColor(k.Color("status-lime-text"))
					k.Text(c.Key("p"), d.tr("界面语言、颜色模式、托盘图标、更新渠道、系统边框、运行模式、后台启动、日志等级、遥测开关、同步开关、历史保留与缓存配额、单文件/总大小上限。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
				})
				ui.Column(c.Key("exc")).Basis(240).Grow(1).MinWidth(0).Gap(5).Children(func() {
					ui.Text(c.Key("h"), d.tr("✕ 明确排除（保留本机原值）")).FontSize(12).FontWeight(600).TextColor(k.Color("text-secondary"))
					k.Text(c.Key("p"), d.tr("设备名称与身份密钥、配对凭据、插件配置与私有目录、数据/接收路径、系统登录注册、全局快捷键绑定、更新器独立偏好。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
				})
			})
		}
	})

	// Dedicated Import Preview Diff Card.
	if s.preview != nil {
		accentFg := k.Color("accent-600")
		if k.Dark {
			accentFg = k.Color("accent-400")
		}
		ui.Column(c.Key("preview-card")).Radius(12).Background(k.CardSurface()).
			Border(1.5, k.Color("accent-500").Alpha(.42)).
			Dividers(1, k.Color("separator-border")).Clip().MinWidth(0).Children(func() {
			ui.Row(c.Key("head")).Padding(11, 16).Gap(8).AlignItems(ui.Center).Justify(ui.SpaceBetween).
				Background(k.Color("accent-50")).MinWidth(0).Children(func() {
				ui.Row(c.Key("left")).Gap(8).AlignItems(ui.Center).Children(func() {
					k.Text(c.Key("title"), d.tr("导入预览"), desktopkit.TextProps{Emphasized: true})
					settingsPill(c.Key("count"), k, fmt.Sprintf(d.tr("%d 项变更"), len(s.preview.changes)), desktopkit.StatusInfo, false, false)
				})
			})
			for _, change := range s.preview.changes {
				change := change
				var extra func()
				if change.key == "desktop.system_window_frame" {
					extra = func() {
						settingsPill(c.Key("restart"), k, d.tr("需重启"), desktopkit.StatusWarning, false, false)
					}
				}
				k.SettingsRow(c.Key("ch-"+change.key), desktopkit.SettingsRowProps{
					Label: d.tr(portableSettingLabels[change.key]),
				}, extra, func() {
					ui.Row(c.Key("diff")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
						k.Text(c.Key("old"), d.backupDisplayValue(change.key, change.before), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
						k.Text(c.Key("arr"), "→", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
						ui.Text(c.Key("new"), d.backupDisplayValue(change.key, change.after)).FontSize(12.5).FontWeight(600).TextColor(accentFg)
					})
				})
			}
			settingsFooterBar(c.Key("footer"), k, d.tr("预览阶段尚未写入任何配置。确认后仅替换上方列出的可移动设置。"), false, func() {
				if k.Button(c.Key("cancel-settings-preview"), d.tr("取消导入"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: s.busy || l.busy,
				}).Clicked() {
					d.cancelBackupImport()
				}
				if k.Button(c.Key("confirm-settings-preview"), d.tr("应用导入"), desktopkit.ButtonProps{
					Variant:  desktopkit.Primary,
					Size:     desktopkit.SizeSmall,
					Disabled: disabled || len(s.preview.changes) == 0,
				}).Clicked() {
					s.confirmOpen = true
				}
			})
		})
	}

	if s.confirmOpen {
		result := k.AlertDialog(c.Key("confirm-settings-import"), desktopkit.AlertDialogProps{Open: &s.confirmOpen, Title: d.tr("导入配置？"), Description: d.tr("导入将替换预览中的设置。部分设置需要重启。"), ConfirmLabel: d.tr("确认导入"), CancelLabel: d.tr("取消"), Destructive: true, ConfirmDisabled: disabled}, nil)
		if result.Confirmed {
			d.confirmBackupImport()
		}
	}
}
