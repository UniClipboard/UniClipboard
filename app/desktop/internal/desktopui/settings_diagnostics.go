package desktopui

import (
	"context"
	"fmt"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/observability"
)

// diagnosticsState is owned by the view thread. Workers return immutable values
// through the existing live result queue; they never mutate the view directly.
type diagnosticsState struct {
	levels       observability.LogLevelState
	telemetry    observability.TelemetryState
	loaded, busy bool
	nextRead     time.Time
	err, notice  string
	export       func(context.Context) (string, error)
	openLogs     func(context.Context) error
}

// SetDiagnosticsActions binds Desktop-owned native dialogs and file actions.
// Both callbacks run on a worker and must not access view state. An empty export
// path means cancellation. The host selects the private installation/profile.
func (s *Desktop) SetDiagnosticsActions(export func(context.Context) (string, error), openLogs func(context.Context) error) {
	s.d.diagnostics.export = export
	s.d.diagnostics.openLogs = openLogs
}

func (d *viewState) refreshDiagnostics() {
	l := d.live
	s := &d.diagnostics
	if l == nil || s.busy {
		return
	}
	s.busy = true
	if !l.enqueue(func(ctx context.Context) func() {
		levels, err := l.client.LogLevels(ctx)
		var telemetry observability.TelemetryState
		if err == nil {
			telemetry, err = l.client.Telemetry(ctx)
		}
		return func() {
			s.busy = false
			s.nextRead = time.Now().Add(5 * time.Second)
			if err != nil {
				s.loaded = false
				s.err = liveErrorMessage(err)
				d.toastWarning(d.tr(s.err), desktopkit.ToastOptions{ID: "diagnostics-feedback"})
				return
			}
			s.levels = levels
			s.telemetry = telemetry
			s.loaded = true
			s.err = ""
		}
	}) {
		s.busy = false
		s.err = d.tr("诊断请求未启动，请重试。")
		d.toastWarning(s.err, desktopkit.ToastOptions{ID: "diagnostics-feedback"})
	}
}

func (d *viewState) setDiagnosticsOverride(target string, reset bool) {
	l := d.live
	s := &d.diagnostics
	if l == nil || s.busy || l.busy {
		return
	}
	s.busy = true
	s.notice = ""
	if !l.enqueue(func(ctx context.Context) func() {
		var levels observability.LogLevelState
		var err error
		if reset {
			levels, err = l.client.ResetLogLevel(ctx, target)
		} else {
			levels, err = l.client.SetLogLevel(ctx, target, "debug", 900)
		}
		return func() {
			s.busy = false
			if err != nil {
				s.err = liveErrorMessage(err)
				s.loaded = false
				d.toastError(d.tr(s.err), desktopkit.ToastOptions{ID: "diagnostics-feedback"})
				return
			}
			s.levels = levels
			s.loaded = true
			s.err = ""
			s.nextRead = time.Time{}
		}
	}) {
		s.busy = false
		s.err = d.tr("诊断请求未启动，请重试。")
		d.toastWarning(s.err, desktopkit.ToastOptions{ID: "diagnostics-feedback"})
	}
}

func (d *viewState) runDiagnosticsAction(export bool) {
	l := d.live
	s := &d.diagnostics
	if l == nil || s.busy || l.busy {
		return
	}
	if (export && s.export == nil) || (!export && s.openLogs == nil) {
		s.err = d.tr("当前宿主不支持此操作。")
		d.toastWarning(s.err, desktopkit.ToastOptions{ID: "diagnostics-feedback"})
		return
	}
	s.busy = true
	s.notice = ""
	s.err = ""
	exportAction, openAction := s.export, s.openLogs
	// A native save panel may remain open while the user chooses a destination.
	// Use a separate worker so a pending native panel cannot stall the shared
	// service/config job queue. Completion still uses the view-owned queue.
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Minute)
		defer cancel()
		var path string
		var err error
		if export {
			path, err = exportAction(ctx)
		} else {
			err = openAction(ctx)
		}
		if ctx.Err() != nil {
			err = ctx.Err()
		}
		apply := func() {
			s.busy = false
			if err != nil {
				s.err = d.tr("诊断操作失败，请重试。")
				d.toastError(s.err, desktopkit.ToastOptions{ID: "diagnostics-feedback"})
				return
			}
			if export {
				if path == "" {
					s.notice = d.tr("已取消导出。")
					d.toastInfo(s.notice, desktopkit.ToastOptions{ID: "diagnostics-feedback"})
				} else {
					s.notice = d.tr("日志已导出：") + path
					d.toastSuccess(d.tr("日志已导出"), desktopkit.ToastOptions{ID: "diagnostics-feedback", Description: path})
				}
			} else {
				s.notice = d.tr("已请求打开日志目录。")
				d.toastInfo(s.notice, desktopkit.ToastOptions{ID: "diagnostics-feedback"})
			}
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func formatOverrideRemaining(expiresMs int64) string {
	rem := time.Until(time.UnixMilli(expiresMs))
	if rem < 0 {
		rem = 0
	}
	secs := int(rem.Seconds())
	return fmt.Sprintf("%02d:%02d", secs/60, secs%60)
}

func (d *viewState) diagnosticsSettings(c *ui.Context, k desktopkit.Kit, cfg *config.Config, changed *bool) {
	s := &d.diagnostics
	l := d.live
	if !s.busy && time.Now().After(s.nextRead) {
		d.refreshDiagnostics()
	}
	disabled := l.showBusy() || s.busy

	// 1. Unified Telemetry & Privacy group.
	k.SettingsGroup(c.Key("diagnostic-signals"), desktopkit.SettingsGroupProps{
		Title: d.tr("隐私与遥测"),
		Trailing: func() {
			switch {
			case !s.loaded:
				settingsPill(c.Key("tel-state"), k, d.tr("诊断状态不可用"), desktopkit.StatusWarning, true, false)
			case s.telemetry.Effective:
				settingsPill(c.Key("tel-state"), k, d.tr("当前上传：已启用"), desktopkit.StatusPositive, true, false)
			default:
				settingsPill(c.Key("tel-state"), k, d.tr("当前上传：未启用"), desktopkit.StatusNeutral, true, false)
			}
			if k.Button(c.Key("refresh-diagnostic-status"), d.tr("刷新诊断状态"), desktopkit.ButtonProps{
				Variant:  desktopkit.Subtle,
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconRetry,
				Disabled: disabled,
				Busy:     s.busy,
			}).Clicked() {
				d.refreshDiagnostics()
			}
		},
	}, func() {
		k.SettingsRow(c.Key("telemetry"), desktopkit.SettingsRowProps{
			Label:       d.tr("发送使用数据与诊断"),
			Description: d.tr("总开关关闭后立即停止遥测上传，并保留下方各信号的选择。"),
		}, nil, func() {
			if k.Switch(c, &cfg.Telemetry.Enabled, desktopkit.SwitchProps{
				Label:    d.tr("发送使用数据与诊断"),
				Disabled: disabled,
			}).Changed() && !l.busy {
				*changed = true
			}
		})
		if !cfg.Telemetry.Enabled {
			settingsNoteRow(c.Key("master-off"), k, d.tr("总开关已关闭：以下 4 项信号当前均暂停上传，但你的偏好开关已被保留。"))
		}
		signals := []struct {
			id, label, desc string
			value           *bool
			state           observability.SignalState
		}{
			{"upload-logs", "诊断日志", "上传达到设定等级的脱敏运行日志", &cfg.Telemetry.Logs.Enabled, s.telemetry.Logs},
			{"upload-metrics", "性能指标", "同步耗时、传输成功率与连接恢复指标", &cfg.Telemetry.Metrics.Enabled, s.telemetry.Metrics},
			{"upload-traces", "采样追踪", "低频采样的流水线与同步调用链", &cfg.Telemetry.Traces.Enabled, s.telemetry.Traces},
			{"upload-analytics", "匿名使用事件", "使用独立随机档案标识，不含剪贴板内容", &cfg.Telemetry.Analytics.Enabled, s.telemetry.Analytics.SignalState},
		}
		for _, sig := range signals {
			sig := sig
			k.SettingsRow(c.Key(sig.id), desktopkit.SettingsRowProps{
				Label:       d.tr(sig.label),
				Description: d.tr(sig.desc),
				Dim:         !cfg.Telemetry.Enabled,
			}, func() {
				if !s.loaded {
					settingsPill(c.Key("st"), k, d.tr("状态不可用"), desktopkit.StatusNeutral, false, false)
					return
				}
				tone := desktopkit.StatusNeutral
				if sig.state.State == "active" {
					tone = desktopkit.StatusPositive
				}
				label := fmt.Sprintf("%s · %d %s", sig.state.State, sig.state.Dropped, d.tr("丢弃"))
				settingsPill(c.Key("st"), k, label, tone, false, true)
			}, func() {
				if k.Switch(c, sig.value, desktopkit.SwitchProps{
					Label:    d.tr(sig.label),
					Disabled: disabled,
				}).Changed() && !l.busy {
					*changed = true
				}
			})
		}
	})

	// 2. Log levels + contextual 15-minute debug overrides.
	k.SettingsGroup(c.Key("diagnostic-levels"), desktopkit.SettingsGroupProps{
		Title:  d.tr("诊断日志"),
		Footer: d.tr("临时调试覆盖仅在服务内存中生效 15 分钟，不改写配置文件，到期后自动恢复已保存等级。"),
	}, func() {
		if !s.loaded {
			settingsNoteRow(c.Key("levels-unavail"), k, d.tr("无法读取服务当前有效日志等级，临时调试操作暂不可用。"))
		}
		levelRows := []struct {
			target, id, label, desc string
			value                   *string
			state                   observability.LevelState
		}{
			{"local", "local-log-level", "本机日志等级", "写入本机日志目录的持久化日志级别，独立于上传日志等级。", &cfg.Logging.Level, s.levels.Local},
			{"remote", "remote-log-level", "上传日志等级", "随遥测上传的日志级别，受上方遥测总开关与日志开关控制。", &cfg.Telemetry.Logs.Level, s.levels.Remote},
		}
		levelItems := []desktopkit.SelectItem{
			{Value: "debug", Label: "debug"},
			{Value: "info", Label: "info"},
			{Value: "warn", Label: "warn"},
			{Value: "error", Label: "error"},
		}
		for _, row := range levelRows {
			row := row
			overrideActive := s.loaded && row.state.Expires != nil
			k.SettingsRow(c.Key(row.id), desktopkit.SettingsRowProps{
				Label:       d.tr(row.label),
				Description: d.tr(row.desc),
			}, func() {
				if !s.loaded {
					return
				}
				if overrideActive {
					text := fmt.Sprintf("%s %s · %s %s", d.tr("临时调试"), row.state.Effective, d.tr("剩余"), formatOverrideRemaining(*row.state.Expires))
					settingsPill(c.Key("eff"), k, text, desktopkit.StatusWarning, true, true)
				} else {
					settingsPill(c.Key("eff"), k, d.tr("有效：")+row.state.Effective, desktopkit.StatusNeutral, false, true)
				}
			}, func() {
				if k.Select(c.Key("sel"), desktopkit.SelectProps{
					Label:    d.tr(row.label),
					Value:    row.value,
					Disabled: disabled,
					Items:    levelItems,
				}).Changed && !l.busy {
					*changed = true
				}
				if overrideActive {
					if k.Button(c.Key("reset-"+row.target), d.tr("恢复默认"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: disabled || !s.loaded,
					}).Clicked() {
						d.setDiagnosticsOverride(row.target, true)
					}
				} else {
					if k.Button(c.Key("debug-"+row.target), d.tr("临时调试 15m"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: disabled || !s.loaded,
					}).Clicked() {
						d.setDiagnosticsOverride(row.target, false)
					}
				}
			})
		}
	})

	// 3. Diagnostic files.
	k.SettingsGroup(c.Key("diagnostic-files"), desktopkit.SettingsGroupProps{Title: d.tr("诊断文件")}, func() {
		k.SettingsRow(c.Key("export-row"), desktopkit.SettingsRowProps{
			Label:       d.tr("导出诊断日志"),
			Description: d.tr("仅导出本机保留的脱敏日志与校验清单（.tar.gz），不含配置、凭据或剪贴板历史。"),
		}, nil, func() {
			if k.Button(c.Key("export-diagnostic-logs"), d.tr("导出诊断日志"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconDownload,
				Disabled: disabled || s.export == nil,
			}).Clicked() {
				d.runDiagnosticsAction(true)
			}
		})
		k.SettingsRow(c.Key("open-row"), desktopkit.SettingsRowProps{
			Label:       d.tr("本机日志目录"),
			Description: d.tr("在系统文件管理器中打开当前配置档案的日志目录。"),
		}, nil, func() {
			if k.Button(c.Key("open-diagnostic-logs"), d.tr("打开日志目录"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconFolder,
				Disabled: disabled || s.openLogs == nil,
			}).Clicked() {
				d.runDiagnosticsAction(false)
			}
		})
	})
}
