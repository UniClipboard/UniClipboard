package desktopui

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type cloudAccountState struct {
	loadedDir      string
	loadedMod      time.Time
	loadedSize     int64
	hasFile        bool
	lastStatAt     time.Time
	signedIn       bool
	reauthRequired bool
	session        cloudauth.Session
	busy           bool
	busySince      time.Time
	busyLabel      string
	err            string
	cancel         context.CancelFunc

	loginFn    func(context.Context, cloudauth.LoginOptions) (cloudauth.Session, error)
	httpClient *http.Client
}

func (c cloudAccountState) showBusy() bool {
	if !c.busy {
		return false
	}
	if c.busySince.IsZero() {
		return true
	}
	return time.Since(c.busySince) >= foregroundLoadingIndicatorDelay
}

func hasRelayFallbackKey(cfg config.Config) bool {
	return cfg.Sync.Relay.AccessKeySet ||
		cfg.Sync.Relay.EnvAccessKeySet ||
		strings.TrimSpace(cfg.Sync.Relay.AccessKey) != ""
}

func (d *viewState) newCloudManager(profileDir, deviceName string) *cloudauth.Manager {
	return cloudauth.NewManager(profileDir, cloudauth.ManagerOptions{
		HTTPClient: d.cloud.httpClient,
		DeviceName: func() string { return deviceName },
	})
}

func (d *viewState) cloudProfileDir() string {
	if d.live == nil || strings.TrimSpace(d.live.config.Path) == "" {
		return ""
	}
	return filepath.Dir(d.live.config.Path)
}

func (d *viewState) ensureCloudSessionLoaded() {
	dir := d.cloudProfileDir()
	if dir == "" {
		return
	}
	now := time.Now()
	if d.cloud.loadedDir == dir && !d.cloud.lastStatAt.IsZero() && now.Sub(d.cloud.lastStatAt) < 400*time.Millisecond {
		return
	}
	d.cloud.lastStatAt = now
	fi, err := os.Stat(cloudauth.SessionPath(dir))
	if err != nil {
		if d.cloud.loadedDir != dir || d.cloud.hasFile || d.cloud.signedIn || d.cloud.reauthRequired {
			d.cloud.loadedDir = dir
			d.cloud.hasFile = false
			d.cloud.loadedMod = time.Time{}
			d.cloud.loadedSize = 0
			d.cloud.signedIn = false
			d.cloud.reauthRequired = false
			d.cloud.session = cloudauth.Session{}
		}
		return
	}
	if d.cloud.loadedDir == dir && d.cloud.hasFile && fi.ModTime().Equal(d.cloud.loadedMod) && fi.Size() == d.cloud.loadedSize {
		return
	}
	d.cloud.loadedDir = dir
	d.cloud.hasFile = true
	d.cloud.loadedMod = fi.ModTime()
	d.cloud.loadedSize = fi.Size()
	if sess, ok, lerr := cloudauth.LoadSession(dir); lerr == nil && ok {
		state := sess.EffectiveState()
		d.cloud.signedIn = state == cloudauth.SessionStateActive
		d.cloud.reauthRequired = state == cloudauth.SessionStateReauthRequired
		d.cloud.session = sess
	} else {
		d.cloud.signedIn = false
		d.cloud.reauthRequired = false
		d.cloud.session = cloudauth.Session{}
	}
}

func (d *viewState) cloudSettingsContent(c *ui.Context, k desktopkit.Kit, cfg *config.Config) {
	d.cloudAccountGroup(c, k, cfg)
	d.cloudServicesGroup(c, k, cfg)
}

func (d *viewState) cloudAccountGroup(c *ui.Context, k desktopkit.Kit, cfg *config.Config) {
	d.ensureCloudSessionLoaded()
	l := d.live

	trailing := func() {
		switch {
		case d.cloud.showBusy():
			settingsPill(c.Key("cloud-pill"), k, d.cloud.busyLabel, desktopkit.StatusWarning, true, false)
		case d.cloud.signedIn:
			settingsPill(c.Key("cloud-pill"), k, d.tr("已登录"), desktopkit.StatusPositive, true, false)
		case d.cloud.reauthRequired:
			settingsPill(c.Key("cloud-pill"), k, d.tr("需重新登录"), desktopkit.StatusWarning, true, false)
		default:
			settingsPill(c.Key("cloud-pill"), k, d.tr("未登录"), desktopkit.StatusNeutral, false, false)
		}
	}

	k.SettingsGroup(c.Key("cloud-account"), desktopkit.SettingsGroupProps{
		Title:    d.tr("账号"),
		Trailing: trailing,
	}, func() {
		switch {
		case d.cloud.signedIn:
			k.SettingsRow(c.Key("cloud-user"), desktopkit.SettingsRowProps{
				Label:       d.cloud.session.User.DisplayLabel(),
				Description: d.tr("已连接，可使用云服务功能。"),
			}, nil, func() {
				if k.Button(c.Key("cloud-logout"), d.tr("退出登录"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Variant:  desktopkit.Destructive,
					Disabled: l.showBusy() || d.cloud.showBusy(),
				}).Clicked() && !l.busy && !d.cloud.busy {
					d.startCloudLogout(*cfg)
				}
			})
		case d.cloud.reauthRequired:
			accountLabel := d.cloud.session.User.DisplayLabel()
			if accountLabel == "" {
				accountLabel = d.tr("云服务账号")
			}
			k.SettingsRow(c.Key("cloud-reauth"), desktopkit.SettingsRowProps{
				Label:       accountLabel,
				Description: d.tr("登录已失效，请重新登录以恢复官方服务。"),
			}, nil, func() {
				if k.Button(c.Key("cloud-reauth-btn"), d.tr("重新登录"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Variant:  desktopkit.Primary,
					Disabled: l.showBusy() || d.cloud.showBusy(),
				}).Clicked() && !l.busy && !d.cloud.busy {
					d.startCloudLogin(*cfg, true)
				}
			})
		default:
			k.SettingsRow(c.Key("cloud-signin"), desktopkit.SettingsRowProps{
				Label:       d.tr("登录账号"),
				Description: d.tr("登录后可使用官方中转等云功能。"),
			}, nil, func() {
				if d.cloud.showBusy() {
					if k.Button(c.Key("cloud-cancel"), d.tr("取消"), desktopkit.ButtonProps{
						Size: desktopkit.SizeSmall,
					}).Clicked() {
						if d.cloud.cancel != nil {
							d.cloud.cancel()
							d.cloud.cancel = nil
						}
						d.cloud.busy = false
						d.cloud.busySince = time.Time{}
						d.cloud.busyLabel = ""
						d.cloud.err = ""
					}
				} else if k.Button(c.Key("cloud-login-btn"), d.tr("登录"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Variant:  desktopkit.Primary,
					Disabled: l.showBusy() || d.cloud.showBusy(),
				}).Clicked() && !l.busy && !d.cloud.busy {
					d.startCloudLogin(*cfg, true)
				}
			})
		}

		if d.cloud.err != "" {
			settingsNoteRow(c.Key("cloud-err"), k, d.cloud.err)
		}
	})
}

func (d *viewState) cloudServicesGroup(c *ui.Context, k desktopkit.Kit, cfg *config.Config) {
	l := d.live
	customRelay := strings.TrimSpace(cfg.Sync.Relay.URL) != ""
	hasFallback := hasRelayFallbackKey(*cfg)
	officialReady := !customRelay && cfg.Sync.Relay.Enabled && (hasFallback || (d.cloud.signedIn && d.cloud.session.HasActiveCredential(time.Now())))

	k.SettingsGroup(c.Key("cloud-services"), desktopkit.SettingsGroupProps{
		Title: d.tr("可用服务"),
	}, func() {
		switch {
		case customRelay:
			k.SettingsRow(c.Key("cloud-svc-relay"), desktopkit.SettingsRowProps{
				Label:       d.tr("混合中转"),
				Description: d.tr("当前使用自建服务，可在同步设置中调整。"),
			}, nil, func() {
				if k.Button(c.Key("cloud-svc-relay-open"), d.tr("前往同步"), desktopkit.ButtonProps{
					Size: desktopkit.SizeSmall,
				}).Clicked() {
					d.settings.Value = "sync"
				}
			})
		case officialReady:
			k.SettingsRow(c.Key("cloud-svc-relay"), desktopkit.SettingsRowProps{
				Label:       d.tr("混合中转"),
				Description: d.tr("设备离线时加密暂存，上线后自动接收。"),
			}, nil, func() {
				ui.Row(c.Key("cloud-svc-relay-actions")).Gap(8).AlignItems(ui.Center).Children(func() {
					settingsPill(c.Key("cloud-svc-relay-pill"), k, d.tr("已开启"), desktopkit.StatusPositive, true, false)
					if k.Button(c.Key("cloud-svc-relay-open"), d.tr("设置"), desktopkit.ButtonProps{
						Size: desktopkit.SizeSmall,
					}).Clicked() {
						d.settings.Value = "sync"
					}
				})
			})
		case d.cloud.signedIn:
			k.SettingsRow(c.Key("cloud-svc-relay"), desktopkit.SettingsRowProps{
				Label:       d.tr("混合中转"),
				Description: d.tr("设备离线时加密暂存，上线后自动接收。"),
			}, nil, func() {
				if k.Button(c.Key("cloud-svc-relay-enable"), d.tr("开启"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Variant:  desktopkit.Primary,
					Disabled: l.showBusy() || d.cloud.showBusy(),
				}).Clicked() && !l.busy && !d.cloud.busy {
					d.provisionCloudRelay(*cfg)
				}
			})
		case d.cloud.reauthRequired:
			k.SettingsRow(c.Key("cloud-svc-relay"), desktopkit.SettingsRowProps{
				Label:       d.tr("混合中转"),
				Description: d.tr("登录已失效，请重新登录。"),
				Dim:         true,
			}, nil, func() {
				settingsPill(c.Key("cloud-svc-relay-pill"), k, d.tr("需重新登录"), desktopkit.StatusWarning, true, false)
			})
		default:
			k.SettingsRow(c.Key("cloud-svc-relay"), desktopkit.SettingsRowProps{
				Label:       d.tr("混合中转"),
				Description: d.tr("设备离线时加密暂存，登录后即可开启。"),
				Dim:         true,
			}, nil, func() {
				settingsPill(c.Key("cloud-svc-relay-pill"), k, d.tr("需登录"), desktopkit.StatusNeutral, false, false)
			})
		}
	})
}

func (d *viewState) startCloudLogin(curCfg config.Config, autoEnableRelay bool) {
	if d.live == nil || d.cloud.busy {
		return
	}
	baseCtx := d.live.ctx
	if baseCtx == nil {
		baseCtx = context.Background()
	}
	ctx, cancel := context.WithTimeout(baseCtx, 5*time.Minute)
	d.cloud.busy = true
	d.cloud.busySince = time.Now()
	d.cloud.busyLabel = d.tr("等待浏览器登录…")
	d.cloud.err = ""
	d.cloud.cancel = cancel

	loginFn := d.cloud.loginFn
	if loginFn == nil {
		loginFn = cloudauth.Login
	}
	profileDir := d.cloudProfileDir()
	deviceName := strings.TrimSpace(curCfg.DeviceName)
	useOfficialRelay := autoEnableRelay && strings.TrimSpace(curCfg.Sync.Relay.URL) == ""
	mgr := d.newCloudManager(profileDir, deviceName)
	httpCli := d.cloud.httpClient
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		sess, err := loginFn(ctx, cloudauth.LoginOptions{
			DeviceName:  deviceName,
			HTTPClient:  httpCli,
			OpenBrowser: d.openExternalURL,
		})
		if err == nil {
			sess, err = mgr.SetSession(sess)
		}
		var cred cloudauth.DeviceCredential
		var hasCred bool
		if err == nil && useOfficialRelay {
			var rerr error
			cred, sess, rerr = mgr.EnsureDeviceCredential(ctx)
			if rerr == nil {
				hasCred = true
			} else {
				err = rerr
			}
		}
		// Snapshot cancellation before defer cancel() fires on goroutine exit,
		// because live.results is buffered and post() runs later on the view thread.
		canceled := ctx.Err() == context.Canceled
		post := func() {
			d.cloud.busy = false
			d.cloud.busySince = time.Time{}
			d.cloud.busyLabel = ""
			d.cloud.cancel = nil
			d.cloud.lastStatAt = time.Time{}
			if sess.EffectiveState() == cloudauth.SessionStateActive {
				d.cloud.signedIn = true
				d.cloud.reauthRequired = false
				d.cloud.session = sess
			} else if sess.EffectiveState() == cloudauth.SessionStateReauthRequired {
				d.cloud.signedIn = false
				d.cloud.reauthRequired = true
				d.cloud.session = sess
			}
			if err != nil {
				if canceled {
					return
				}
				d.cloud.err = fmt.Sprintf(d.tr("登录失败：%v"), err)
				return
			}
			d.cloud.err = ""
			if hasCred {
				d.applyCloudRelayCredential(cred)
			}
		}
		select {
		case results <- post:
			live.wake()
		case <-baseCtx.Done():
		}
	}()
}

func (d *viewState) provisionCloudRelay(curCfg config.Config) {
	if d.live == nil || d.cloud.busy || !d.cloud.signedIn {
		return
	}
	baseCtx := d.live.ctx
	if baseCtx == nil {
		baseCtx = context.Background()
	}
	ctx, cancel := context.WithTimeout(baseCtx, 30*time.Second)
	d.cloud.busy = true
	d.cloud.busySince = time.Now()
	d.cloud.busyLabel = d.tr("正在配置中转…")
	d.cloud.err = ""
	d.cloud.cancel = cancel

	profileDir := d.cloudProfileDir()
	deviceName := strings.TrimSpace(curCfg.DeviceName)
	mgr := d.newCloudManager(profileDir, deviceName)
	sess := d.cloud.session
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		if profileDir == "" {
			_, _ = mgr.SetSession(sess)
		}
		cred, updated, err := mgr.EnsureDeviceCredential(ctx)
		post := func() {
			d.cloud.busy = false
			d.cloud.busySince = time.Time{}
			d.cloud.busyLabel = ""
			d.cloud.cancel = nil
			d.cloud.lastStatAt = time.Time{}
			if err != nil {
				if updated.EffectiveState() == cloudauth.SessionStateReauthRequired {
					d.cloud.signedIn = false
					d.cloud.reauthRequired = true
					d.cloud.session = updated
				}
				d.cloud.err = fmt.Sprintf(d.tr("配置中转失败：%v"), err)
				return
			}
			d.cloud.signedIn = true
			d.cloud.reauthRequired = false
			d.cloud.session = updated
			d.cloud.err = ""
			d.applyCloudRelayCredential(cred)
		}
		select {
		case results <- post:
			live.wake()
		case <-baseCtx.Done():
		}
	}()
}

func (d *viewState) startCloudLogout(curCfg config.Config) {
	if d.live == nil || d.cloud.busy || !d.cloud.signedIn {
		return
	}
	baseCtx := d.live.ctx
	if baseCtx == nil {
		baseCtx = context.Background()
	}
	ctx, cancel := context.WithTimeout(baseCtx, 15*time.Second)
	d.cloud.busy = true
	d.cloud.busySince = time.Now()
	d.cloud.busyLabel = d.tr("正在退出…")
	d.cloud.err = ""
	d.cloud.cancel = cancel

	profileDir := d.cloudProfileDir()
	mgr := d.newCloudManager(profileDir, "")
	sess := d.cloud.session
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		if profileDir == "" {
			_, _ = mgr.SetSession(sess)
		}
		_ = mgr.Logout(ctx)
		post := func() {
			d.cloud.busy = false
			d.cloud.busySince = time.Time{}
			d.cloud.busyLabel = ""
			d.cloud.cancel = nil
			d.cloud.lastStatAt = time.Time{}
			d.cloud.signedIn = false
			d.cloud.reauthRequired = false
			d.cloud.session = cloudauth.Session{}
			d.cloud.err = ""
			liveCfg := d.live.config.Config
			if strings.TrimSpace(liveCfg.Sync.Relay.URL) == "" && liveCfg.Sync.Relay.Enabled && !hasRelayFallbackKey(liveCfg) {
				nextCfg := liveCfg
				nextCfg.Sync.Relay.Enabled = false
				d.saveLiveConfig(nextCfg)
			}
		}
		select {
		case results <- post:
			live.wake()
		case <-baseCtx.Done():
		}
	}()
}

func (d *viewState) applyCloudRelayCredential(_ cloudauth.DeviceCredential) {
	nextCfg := d.live.config.Config
	nextCfg.Sync.Relay.Enabled = true
	// Keep sync.relay.url empty so official mode stays official even when Cloud
	// advertises a non-default relay origin inside cloud-session.json.
	nextCfg.Sync.Relay.URL = ""
	d.relay.urlDraft = nextCfg.Sync.Relay.URL
	d.relay.urlSynced = nextCfg.Sync.Relay.URL
	d.saveLiveConfig(nextCfg)
}
