package desktopui

import (
	"context"
	"fmt"
	"net/http"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

type cloudAccountState struct {
	loadedDir string
	signedIn  bool
	session   cloudauth.Session
	busy      bool
	busyLabel string
	err       string
	cancel    context.CancelFunc

	loginFn       func(context.Context, cloudauth.LoginOptions) (cloudauth.Session, error)
	createRelayFn func(context.Context, *http.Client, cloudauth.Session, string) (cloudauth.RelayCredential, cloudauth.Session, error)
	revokeRelayFn func(context.Context, *http.Client, cloudauth.Session, string) error
	logoutFn      func(context.Context, *http.Client, cloudauth.Session) error
}

func (d *viewState) cloudProfileDir() string {
	if d.live == nil || strings.TrimSpace(d.live.config.Path) == "" {
		return ""
	}
	return filepath.Dir(d.live.config.Path)
}

func (d *viewState) ensureCloudSessionLoaded() {
	dir := d.cloudProfileDir()
	if dir == "" || d.cloud.loadedDir == dir {
		return
	}
	d.cloud.loadedDir = dir
	if sess, ok, err := cloudauth.LoadSession(dir); err == nil && ok {
		d.cloud.signedIn = true
		d.cloud.session = sess
	} else {
		d.cloud.signedIn = false
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
		if d.cloud.busy {
			settingsPill(c.Key("cloud-pill"), k, d.cloud.busyLabel, desktopkit.StatusWarning, true, false)
		} else if d.cloud.signedIn {
			settingsPill(c.Key("cloud-pill"), k, d.tr("已登录"), desktopkit.StatusPositive, true, false)
		} else {
			settingsPill(c.Key("cloud-pill"), k, d.tr("未登录"), desktopkit.StatusNeutral, false, false)
		}
	}

	k.SettingsGroup(c.Key("cloud-account"), desktopkit.SettingsGroupProps{
		Title:    d.tr("账号"),
		Trailing: trailing,
	}, func() {
		if d.cloud.signedIn {
			k.SettingsRow(c.Key("cloud-user"), desktopkit.SettingsRowProps{
				Label:       d.cloud.session.User.DisplayLabel(),
				Description: d.tr("已连接，可使用云服务功能。"),
			}, nil, func() {
				if k.Button(c.Key("cloud-logout"), d.tr("退出登录"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Variant:  desktopkit.Destructive,
					Disabled: l.busy || d.cloud.busy,
				}).Clicked() {
					d.startCloudLogout(*cfg)
				}
			})
		} else {
			k.SettingsRow(c.Key("cloud-signin"), desktopkit.SettingsRowProps{
				Label:       d.tr("登录账号"),
				Description: d.tr("登录后可使用官方中转等云功能。"),
			}, nil, func() {
				if d.cloud.busy {
					if k.Button(c.Key("cloud-cancel"), d.tr("取消"), desktopkit.ButtonProps{
						Size: desktopkit.SizeSmall,
					}).Clicked() {
						if d.cloud.cancel != nil {
							d.cloud.cancel()
							d.cloud.cancel = nil
						}
						d.cloud.busy = false
						d.cloud.busyLabel = ""
						d.cloud.err = ""
					}
				} else if k.Button(c.Key("cloud-login-btn"), d.tr("登录"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Variant:  desktopkit.Primary,
					Disabled: l.busy,
				}).Clicked() {
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
	officialReady := !customRelay && cfg.Sync.Relay.Enabled && cfg.Sync.Relay.AccessKeySet

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
		case d.cloud.signedIn && officialReady:
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
					Disabled: l.busy || d.cloud.busy,
				}).Clicked() {
					d.provisionCloudRelay(*cfg)
				}
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
	d.cloud.busyLabel = d.tr("等待浏览器登录…")
	d.cloud.err = ""
	d.cloud.cancel = cancel

	loginFn := d.cloud.loginFn
	if loginFn == nil {
		loginFn = cloudauth.Login
	}
	createRelayFn := d.cloud.createRelayFn
	if createRelayFn == nil {
		createRelayFn = cloudauth.CreateRelayAPIKey
	}
	profileDir := d.cloudProfileDir()
	deviceName := strings.TrimSpace(curCfg.DeviceName)
	useOfficialRelay := autoEnableRelay && strings.TrimSpace(curCfg.Sync.Relay.URL) == ""
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		sess, err := loginFn(ctx, cloudauth.LoginOptions{
			DeviceName:  deviceName,
			OpenBrowser: d.openExternalURL,
		})
		var cred cloudauth.RelayCredential
		var hasCred bool
		if err == nil && useOfficialRelay {
			var rerr error
			cred, sess, rerr = createRelayFn(ctx, nil, sess, deviceName)
			if rerr == nil {
				hasCred = true
			} else {
				err = rerr
			}
		}
		if err == nil && profileDir != "" {
			_ = cloudauth.SaveSession(profileDir, sess)
		}
		post := func() {
			d.cloud.busy = false
			d.cloud.busyLabel = ""
			d.cloud.cancel = nil
			if sess.RefreshToken != "" {
				d.cloud.signedIn = true
				d.cloud.session = sess
			}
			if err != nil {
				if ctx.Err() == context.Canceled {
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
	d.cloud.busyLabel = d.tr("正在配置中转…")
	d.cloud.err = ""
	d.cloud.cancel = cancel

	createRelayFn := d.cloud.createRelayFn
	if createRelayFn == nil {
		createRelayFn = cloudauth.CreateRelayAPIKey
	}
	profileDir := d.cloudProfileDir()
	deviceName := strings.TrimSpace(curCfg.DeviceName)
	sess := d.cloud.session
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		cred, updated, err := createRelayFn(ctx, nil, sess, deviceName)
		if err == nil && profileDir != "" {
			_ = cloudauth.SaveSession(profileDir, updated)
		}
		post := func() {
			d.cloud.busy = false
			d.cloud.busyLabel = ""
			d.cloud.cancel = nil
			if err != nil {
				d.cloud.err = fmt.Sprintf(d.tr("配置中转失败：%v"), err)
				return
			}
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
	d.cloud.busyLabel = d.tr("正在退出…")
	d.cloud.err = ""
	d.cloud.cancel = cancel

	revokeRelayFn := d.cloud.revokeRelayFn
	if revokeRelayFn == nil {
		revokeRelayFn = cloudauth.RevokeRelayAPIKey
	}
	logoutFn := d.cloud.logoutFn
	if logoutFn == nil {
		logoutFn = cloudauth.Logout
	}
	profileDir := d.cloudProfileDir()
	sess := d.cloud.session
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		if sess.RelayKeyID != "" {
			_ = revokeRelayFn(ctx, nil, sess, sess.RelayKeyID)
		}
		_ = logoutFn(ctx, nil, sess)
		if profileDir != "" {
			_ = cloudauth.DeleteSession(profileDir)
		}
		post := func() {
			d.cloud.busy = false
			d.cloud.busyLabel = ""
			d.cloud.cancel = nil
			d.cloud.signedIn = false
			d.cloud.session = cloudauth.Session{}
			d.cloud.err = ""
			if strings.TrimSpace(curCfg.Sync.Relay.URL) == "" && (curCfg.Sync.Relay.Enabled || curCfg.Sync.Relay.AccessKeySet) {
				nextCfg := d.live.config.Config
				nextCfg.Sync.Relay.Enabled = false
				d.clearLiveRelayAccessKey(nextCfg, nil)
			}
		}
		select {
		case results <- post:
			live.wake()
		case <-baseCtx.Done():
		}
	}()
}

func (d *viewState) applyCloudRelayCredential(cred cloudauth.RelayCredential) {
	nextCfg := d.live.config.Config
	nextCfg.Sync.Relay.Enabled = true
	if cred.RelayURL == "" || strings.EqualFold(strings.TrimRight(cred.RelayURL, "/"), relayclient.DefaultURL) {
		nextCfg.Sync.Relay.URL = ""
	} else if norm, nerr := relaycrypto.NormalizeRelayOrigin(cred.RelayURL); nerr == nil {
		nextCfg.Sync.Relay.URL = norm
	}
	nextCfg.Sync.Relay.AccessKey = cred.APIKey
	d.relay.urlDraft = nextCfg.Sync.Relay.URL
	d.relay.urlSynced = nextCfg.Sync.Relay.URL
	d.saveLiveConfig(nextCfg)
}
