package desktopui

import (
	"context"
	"fmt"
	"slices"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// storageSettingsState contains aggregate ledger data only. It does not retain
// cache identifiers, filenames, native paths or raw service errors.
type storageSettingsState struct {
	requested, loaded, loading bool
	storedBytes, reservedBytes int64
	fileCount, heldCount       int
	customDestination          bool
	message                    string
	openDirectory              func() error
	opening                    bool
	openMessage                string
}

// SetStorageDirectoryOpener binds the host-owned profile directory action.
// Configure the callback on the view thread; it runs on the existing I/O worker.
func (s *Desktop) SetStorageDirectoryOpener(open func() error) {
	s.d.storage.openDirectory = open
}

func (d *viewState) refreshStorageUsage() {
	l := d.live
	if l == nil || d.storage.loading {
		return
	}
	d.storage.requested, d.storage.loading = true, true
	client := l.client
	if !l.enqueueWithin(func(ctx context.Context) func() {
		state, err := client.FileCache(ctx)
		return func() {
			d.storage.loading = false
			if err != nil {
				d.storage.message = "无法刷新缓存用量。请重试。"
				return
			}
			held := 0
			for _, file := range state.Files {
				if file.Held {
					held++
				}
			}
			d.storage.storedBytes, d.storage.reservedBytes = state.StoredBytes, state.ReservedBytes
			d.storage.fileCount, d.storage.heldCount = len(state.Files), held
			d.storage.customDestination = state.CustomDestination
			d.storage.loaded, d.storage.message = true, ""
		}
	}, 5*time.Second) {
		d.storage.loading = false
		d.storage.message = "无法刷新缓存用量。请重试。"
	}
}

func (d *viewState) openStorageDirectory() {
	l := d.live
	open := d.storage.openDirectory
	if l == nil || open == nil || d.storage.opening {
		return
	}
	d.storage.opening, d.storage.openMessage = true, ""
	if !l.enqueue(func(context.Context) func() {
		err := open()
		return func() {
			d.storage.opening = false
			if err != nil {
				d.storage.openMessage = "无法打开数据目录。请重试。"
			}
		}
	}) {
		d.storage.opening = false
		d.storage.openMessage = "无法打开数据目录。请重试。"
	}
}

func (d *viewState) storageCacheGroup(c *ui.Context, k desktopkit.Kit, cfg *config.Config, changed *bool) {
	if !d.storage.requested {
		d.refreshStorageUsage()
	}
	s := &d.storage
	l := d.live
	quotaBytes := int64(cfg.FileCache.MaxBytes)
	quotaMB := int(quotaBytes / (1 << 20))

	k.SettingsGroup(c.Key("storage-usage"), desktopkit.SettingsGroupProps{
		Title: d.tr("接收文件缓存"),
		Trailing: func() {
			if k.Button(c.Key("refresh-storage"), d.tr("刷新缓存用量"), desktopkit.ButtonProps{
				Variant:  desktopkit.Subtle,
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconRetry,
				Disabled: s.loading,
				Busy:     s.loading,
			}).Clicked() {
				d.refreshStorageUsage()
			}
		},
		Footer: d.tr("仅统计应用管理的接收文件缓存账本；不包含 SQLite 数据库、日志、自定义接收目录或其他用户文件。"),
	}, func() {
		if s.message != "" {
			msg := d.tr(s.message)
			if s.loaded {
				msg += " " + d.tr("下方显示上次成功读取的用量。")
			}
			settingsStrip(c.Key("read-error"), k, desktopkit.ToneWarning, msg)
		}

		// Visual quota meter + 4-column metric strip.
		ui.Column(c.Key("meter-block")).Padding(14, 16).Gap(10).MinWidth(0).Children(func() {
			if s.loaded {
				var pctStored, pctRes float64
				if quotaBytes > 0 {
					pctStored = min(100, float64(s.storedBytes)*100/float64(quotaBytes))
					pctRes = min(100-pctStored, float64(s.reservedBytes)*100/float64(quotaBytes))
				}
				remPct := max(0, 100-pctStored-pctRes)

				ui.Row(c.Key("quota-head")).Wrap().AlignItems(ui.Baseline).Justify(ui.SpaceBetween).Gap(8).MinWidth(0).Children(func() {
					ui.Row(c.Key("left")).Wrap().AlignItems(ui.Baseline).Gap(6).Children(func() {
						ui.Text(c.Key("primary"), humanBytes(s.storedBytes)).FontSize(18).FontWeight(700).TextColor(k.Color("text-primary"))
						sub := d.tr("已存储")
						if s.reservedBytes > 0 {
							sub += " · + " + humanBytes(s.reservedBytes) + " " + d.tr("传输预留")
						}
						k.Text(c.Key("sub"), sub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					})
					capLabel := fmt.Sprintf("%s %s (%.1f%%)", d.tr("配额"), humanBytes(quotaBytes), pctStored)
					k.Text(c.Key("cap"), capLabel, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
				})

				ui.Row(c.Key("quota-bar")).FillWidth().Height(8).Radius(4).Clip().Background(k.Color("background-tertiary-default")).AccessHidden(true).Children(func() {
					if pctStored > 0 {
						ui.Box(c.Key("stored")).Grow(float32(pctStored)).FillHeight().Background(k.Color("accent-500"))
					}
					if pctRes > 0 {
						ui.Box(c.Key("reserved")).Grow(float32(pctRes)).FillHeight().Background(k.Color("accent-500").Alpha(.38))
					}
					if remPct > 0 {
						ui.Box(c.Key("rem")).Grow(float32(remPct)).FillHeight()
					}
				})

				ui.Row(c.Key("facts")).Wrap().Gap(8).MinWidth(0).Children(func() {
					for _, tile := range []struct{ id, label, value string }{
						{"stored", d.tr("已存储文件"), humanBytes(s.storedBytes)},
						{"reserved", d.tr("传输预留空间"), humanBytes(s.reservedBytes)},
						{"files", d.tr("缓存文件数"), fmt.Sprint(s.fileCount)},
						{"held", d.tr("使用中文件数"), fmt.Sprint(s.heldCount)},
					} {
						ui.Column(c.Key(tile.id)).Basis(140).Grow(1).MinWidth(120).Padding(9, 11).Gap(2).Radius(9).
							Background(k.SubtleCardSurface()).Border(1, k.Color("border-button-default")).Children(func() {
							k.Text(c.Key("l"), tile.label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
							ui.Text(c.Key("v"), tile.value).FontSize(14).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
						})
					}
				})
			} else if s.loading {
				k.Text(c.Key("loading"), d.tr("正在读取缓存用量…"), footnote)
			}
		})

		if s.loaded && s.customDestination {
			settingsStrip(c.Key("custom"), k, desktopkit.ToneInfo, d.tr("当前使用自定义接收目录。该目录由用户管理，不计入上方默认缓存用量。"))
		}

		// Quota selector inside the same cache card.
		k.SettingsRow(c.Key("quota"), desktopkit.SettingsRowProps{
			Label:       d.tr("单设备缓存配额"),
			Description: d.tr("仅限制应用管理的文件缓存上限。"),
		}, nil, func() {
			values := []int{100, 500, 1024, 2048, 4096}
			if !slices.Contains(values, quotaMB) {
				values = append(slices.Clone(values), quotaMB)
			}
			opts := make([]desktopkit.SelectItem, 0, len(values))
			for _, n := range values {
				label := fmt.Sprintf("%d MB", n)
				if n >= 1024 && n%1024 == 0 {
					label = fmt.Sprintf("%d GB", n/1024)
				}
				opts = append(opts, desktopkit.SelectItem{Value: strconv.Itoa(n), Label: label})
			}
			v := strconv.Itoa(quotaMB)
			if k.Select(c, desktopkit.SelectProps{Label: d.tr("单设备缓存配额"), Value: &v, Items: opts, Disabled: l.busy}).Changed {
				if parsed, err := strconv.Atoi(v); err == nil {
					cfg.FileCache.MaxBytes = config.Size(parsed) * (1 << 20)
					*changed = true
				}
			}
		})

		// Receive directory row inside the same cache card.
		inbox := cfg.Inbox
		badgeLabel := d.tr("应用管理")
		badgeTone := desktopkit.StatusNeutral
		if inbox == "" {
			inbox = d.tr("应用管理的默认缓存目录")
		} else {
			badgeLabel = d.tr("用户管理")
			badgeTone = desktopkit.StatusInfo
		}
		k.SettingsRow(c.Key("inbox"), desktopkit.SettingsRowProps{
			Label:       d.tr("接收目录"),
			Description: inbox,
		}, func() {
			settingsPill(c.Key("inbox-badge"), k, badgeLabel, badgeTone, false, false)
		}, nil)
	})
}

func (d *viewState) storageLocalDataGroup(c *ui.Context, k desktopkit.Kit) {
	s := &d.storage
	l := d.live
	k.SettingsGroup(c.Key("storage-directory"), desktopkit.SettingsGroupProps{Title: d.tr("本机数据与迁移")}, func() {
		k.SettingsRow(c.Key("data-dir"), desktopkit.SettingsRowProps{
			Label:       d.tr("数据目录"),
			Description: d.tr("在系统文件管理器中查看当前配置的数据目录。"),
		}, nil, func() {
			if k.Button(c.Key("open-data-directory"), d.tr("打开数据目录"), desktopkit.ButtonProps{
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconFolder,
				Disabled: s.openDirectory == nil || s.opening,
			}).Clicked() {
				d.openStorageDirectory()
			}
		})
		if s.openMessage != "" {
			settingsStrip(c.Key("open-error"), k, desktopkit.ToneWarning, d.tr(s.openMessage))
		}
		if l.openMigration != nil {
			k.SettingsRow(c.Key("migration"), desktopkit.SettingsRowProps{
				Label:       d.tr("从 v1 迁移"),
				Description: d.tr("迁移前会停止当前同步和剪贴板捕获，旧数据会保留。"),
			}, nil, func() {
				if k.Button(c.Key("open-migration"), d.tr("从 v1 迁移"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: l.busy,
				}).Clicked() {
					l.openMigration()
				}
			})
		}
	})
}
