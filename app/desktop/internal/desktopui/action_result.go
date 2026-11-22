package desktopui

import (
	"context"
	"errors"
	"net/url"
	"os/exec"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

type actionResultState struct {
	open        bool
	title       string
	plugin      string
	historyID   string
	text        string
	url         string
	copied      bool
	applyFailed bool
	truncated   bool
	openError   string
}

type actionToastState struct {
	visible   bool
	running   bool
	title     string
	message   string
	tone      desktopkit.NoticeTone
	expiresAt time.Time
}

// validHTTPURL returns a trimmed single-line http(s) URL, or "" if raw is not one.
func validHTTPURL(raw string) string {
	s := strings.TrimSpace(raw)
	if s == "" || strings.ContainsAny(s, "\r\n\t ") {
		return ""
	}
	u, err := url.Parse(s)
	if err != nil || u.Host == "" {
		return ""
	}
	if u.Scheme != "http" && u.Scheme != "https" {
		return ""
	}
	return s
}

func openSystemURL(raw string) error {
	target := validHTTPURL(raw)
	if target == "" {
		return errors.New("only http and https URLs are supported")
	}
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", target)
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", target)
	default:
		cmd = exec.Command("xdg-open", target)
	}
	return cmd.Start()
}

func (d *viewState) openExternalURL(raw string) error {
	target := validHTTPURL(raw)
	if target == "" {
		return errors.New("only http and https URLs are supported")
	}
	if d.openURL != nil {
		return d.openURL(target)
	}
	return openSystemURL(target)
}

func (d *viewState) copyHistoryID(id string, onApplied func()) {
	if d.live == nil || id == "" {
		return
	}
	l := d.live
	if l.busy {
		return
	}
	client := l.client
	request := "desktop-" + newRequestID()
	l.busy = true
	l.message = ""
	if !l.enqueue(func(ctx context.Context) func() {
		_, err := client.RestoreHistory(ctx, id, request)
		return func() {
			l.busy = false
			l.nextRefresh = time.Time{}
			if err != nil {
				l.message = liveErrorMessage(err)
				d.notice = "操作失败"
				return
			}
			l.message = ""
			d.notice = "已复制"
			if onApplied != nil {
				onApplied()
			}
		}
	}) {
		l.busy = false
	}
}

func (d *viewState) focusHistoryItem(id string) {
	if id == "" {
		return
	}
	if slices.ContainsFunc(d.entries, func(e entry) bool { return e.id == id }) {
		d.selectEntry(id)
		d.detailOpen, d.inspector = true, true
		d.list.ScrollTo(id)
		return
	}
	if d.live != nil {
		d.live.pendingSelectID = id
		d.live.nextRefresh = time.Time{}
	}
}

func (d *viewState) cancelRunningAction() {
	if d.live != nil && d.live.actionCancel != nil {
		d.live.actionCancel()
	}
}

func (d *viewState) actionResultDialog(c *ui.Context, k desktopkit.Kit) {
	if !d.actionResult.open {
		return
	}
	open := true
	border := k.Color("separator-border")
	k.Dialog(c, desktopkit.DialogProps{
		Open:        &open,
		Title:       d.actionResult.title,
		Description: d.actionResult.plugin,
		Width:       480,
	}, func() {
		if d.actionResult.copied {
			bg, fg := k.TagTint("green")
			ui.Row(c.Key("copied-badge")).Padding(6, 10).Radius(8).Gap(6).AlignItems(ui.Center).Background(bg).Children(func() {
				k.Icon(c.Key("ico"), desktopkit.IconCopy, 13).TextColor(fg)
				ui.Text(c.Key("txt"), d.tr("已自动复制到剪贴板")).FontSize(12.5).FontWeight(500).TextColor(fg)
			})
		} else if d.actionResult.applyFailed {
			k.InlineNotice(c.Key("apply-warn"), desktopkit.NoticeProps{
				Tone:  desktopkit.ToneWarning,
				Title: d.tr("已保存到历史，但未能写入剪贴板。"),
			})
		}
		if d.actionResult.openError != "" {
			k.InlineNotice(c.Key("open-err"), desktopkit.NoticeProps{
				Tone:  desktopkit.ToneError,
				Title: d.actionResult.openError,
			})
		}
		content := d.actionResult.text
		if content == "" {
			content = d.tr("结果已保存到历史。")
		}
		ui.Box(c.Key("result-card")).Padding(12, 14).Radius(10).Background(k.DrawerInnerSurface()).Border(1, border).MaxHeight(220).Children(func() {
			scroll := k.Scroll(c.Key("result-scroll")).MaxHeight(196)
			scroll.Children(func() {
				txt := ui.Text(c.Key("result-text"), content).FontSize(13)
				if d.actionResult.url != "" {
					txt.Font("monospace").TextColor(k.Color("accent-500"))
				} else {
					txt.TextColor(k.Color("text-primary"))
				}
			})
		})
		if d.actionResult.truncated {
			k.Text(c.Key("trunc"), d.tr("内容较长，仅展示前段，完整内容已保存到历史。"), desktopkit.TextProps{
				Role: desktopkit.RoleCaption,
				Tone: desktopkit.ToneTertiary,
			})
		}
		ui.Row(c.Key("footer")).AlignItems(ui.Center).Gap(8).Children(func() {
			if d.actionResult.historyID != "" {
				if k.Button(c.Key("view-item"), d.tr("查看条目"), desktopkit.ButtonProps{Variant: desktopkit.Link}).Clicked() {
					targetID := d.actionResult.historyID
					open = false
					d.focusHistoryItem(targetID)
				}
			}
			ui.Box(c.Key("spacer")).Grow(1)
			if k.Button(c.Key("close"), d.tr("关闭"), desktopkit.ButtonProps{}).Clicked() {
				open = false
			}
			if d.actionResult.historyID != "" {
				copyLabel := d.tr("复制")
				copyVariant := desktopkit.Primary
				if d.actionResult.copied {
					copyLabel = d.tr("再次复制")
					copyVariant = desktopkit.Standard
				}
				if k.Button(c.Key("copy-result"), copyLabel, desktopkit.ButtonProps{
					Icon:     desktopkit.IconCopy,
					Variant:  copyVariant,
					Disabled: d.live != nil && d.live.busy,
				}).Clicked() {
					targetID := d.actionResult.historyID
					d.copyHistoryID(targetID, func() {
						d.actionResult.copied = true
						d.actionResult.applyFailed = false
					})
				}
			}
			if d.actionResult.url != "" {
				openVariant := desktopkit.Standard
				if d.actionResult.copied {
					openVariant = desktopkit.Primary
				}
				if k.Button(c.Key("open-link"), d.tr("打开链接"), desktopkit.ButtonProps{
					Icon:    desktopkit.IconLink,
					Variant: openVariant,
				}).Clicked() {
					if err := d.openExternalURL(d.actionResult.url); err != nil {
						d.actionResult.openError = d.tr("无法打开链接。")
					} else {
						d.actionResult.openError = ""
					}
				}
			}
		})
	})
	if !open {
		d.actionResult = actionResultState{}
	}
}

func (d *viewState) actionToastOverlay(c *ui.Context, k desktopkit.Kit) {
	if !d.actionToast.visible {
		return
	}
	if !d.actionToast.running && !d.actionToast.expiresAt.IsZero() && time.Now().After(d.actionToast.expiresAt) {
		d.actionToast = actionToastState{}
		return
	}
	border := k.Color("separator-border")
	ui.Row(c.Key("toast-anchor")).Absolute().Left(0).Right(0).Bottom(18).Justify(ui.Center).PassThrough().Children(func() {
		pill := ui.Row(c.Key("toast-pill")).Padding(8, 12).Gap(10).Radius(10).AlignItems(ui.Center).
			Background(k.Color("background-primary-default")).Border(1, border).Role(ui.RoleStatus)
		pill.Children(func() {
			switch {
			case d.actionToast.running:
				k.Icon(c.Key("ico"), desktopkit.IconRetry, 14).TextColor(k.Color("accent-500"))
			case d.actionToast.tone == desktopkit.ToneError:
				ui.Box(c.Key("dot")).Size(8, 8).Radius(4).Background(k.Color("text-error-primary"))
			case d.actionToast.tone == desktopkit.ToneWarning:
				ui.Box(c.Key("dot")).Size(8, 8).Radius(4).Background(k.Color("status-orange-text"))
			default:
				_, fg := k.TagTint("green")
				ui.Box(c.Key("dot")).Size(8, 8).Radius(4).Background(fg)
			}
			ui.Text(c.Key("msg"), d.actionToast.message).FontSize(13).TextColor(k.Color("text-primary")).SingleLine()
			if d.actionToast.running {
				if k.Button(c.Key("cancel-action"), d.tr("取消"), desktopkit.ButtonProps{
					Size:    desktopkit.SizeSmall,
					Variant: desktopkit.Subtle,
				}).Clicked() {
					d.cancelRunningAction()
				}
			} else {
				if k.Button(c.Key("dismiss-toast"), "", desktopkit.ButtonProps{
					Icon:    desktopkit.IconDismiss,
					Label:   d.tr("关闭提示"),
					Size:    desktopkit.SizeSmall,
					Variant: desktopkit.Subtle,
				}).Clicked() {
					d.actionToast = actionToastState{}
				}
			}
		})
	})
}
