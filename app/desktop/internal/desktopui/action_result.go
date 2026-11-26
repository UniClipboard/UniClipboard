package desktopui

import (
	"context"
	"errors"
	"image"
	"net/netip"
	"net/url"
	"os/exec"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/skip2/go-qrcode"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

const (
	maxActionQRURLBytes  = 1024
	actionQRMediumCutoff = 512
	actionQRMinPixels    = 512
)

type actionResultState struct {
	open        bool
	title       string
	plugin      string
	historyID   string
	text        string
	url         string
	qr          *ui.Bitmap
	copied      bool
	applyFailed bool
	truncated   bool
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

// qrScannableURL returns a trimmed http(s) URL suitable for a phone-scannable
// QR code, or "" when raw is not a valid non-loopback URL within length bounds.
func qrScannableURL(raw string) string {
	target := validHTTPURL(raw)
	if target == "" || len(target) > maxActionQRURLBytes {
		return ""
	}
	u, err := url.Parse(target)
	if err != nil {
		return ""
	}
	host := strings.ToLower(u.Hostname())
	if host == "" || host == "localhost" || strings.HasSuffix(host, ".localhost") {
		return ""
	}
	if addr, err := netip.ParseAddr(host); err == nil && (addr.IsLoopback() || addr.IsUnspecified()) {
		return ""
	}
	return target
}

// encodeActionQRImage renders a local, integer-scaled QR code with the standard
// four-module white quiet zone. It never performs network I/O or logs the URL.
func encodeActionQRImage(raw string) *image.NRGBA {
	target := qrScannableURL(raw)
	if target == "" {
		return nil
	}
	level := qrcode.Medium
	if len(target) > actionQRMediumCutoff {
		level = qrcode.Low
	}
	code, err := qrcode.New(target, level)
	if err != nil {
		return nil
	}
	matrix := code.Bitmap()
	n := len(matrix)
	if n == 0 {
		return nil
	}
	scale := max(1, (actionQRMinPixels+n-1)/n)
	side := n * scale
	img := image.NewNRGBA(image.Rect(0, 0, side, side))
	for i := 0; i < len(img.Pix); i += 4 {
		img.Pix[i] = 255
		img.Pix[i+1] = 255
		img.Pix[i+2] = 255
		img.Pix[i+3] = 255
	}
	for row := 0; row < n; row++ {
		line := matrix[row]
		for col := 0; col < len(line); col++ {
			if !line[col] {
				continue
			}
			y0 := row * scale
			x0 := col * scale
			for dy := 0; dy < scale; dy++ {
				off := (y0+dy)*img.Stride + x0*4
				for dx := 0; dx < scale; dx++ {
					p := off + dx*4
					img.Pix[p] = 0
					img.Pix[p+1] = 0
					img.Pix[p+2] = 0
					img.Pix[p+3] = 255
				}
			}
		}
	}
	return img
}

func newActionResultQR(img *image.NRGBA) *ui.Bitmap {
	if img == nil {
		return nil
	}
	return ui.NewBitmap(img)
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
	request := "desktop-" + newRequestID()
	copy := l.copyHistory(id, request)
	l.startBusy("")
	l.message = ""
	if !l.enqueue(func(ctx context.Context) func() {
		err := copy(ctx)
		return func() {
			l.stopBusy()
			l.nextRefresh = time.Time{}
			if err != nil {
				l.message = liveErrorMessage(err)
				d.notice = "操作失败"
				d.toastError(d.tr(l.message), desktopkit.ToastOptions{ID: toastIDLiveMessage})
				return
			}
			l.message = ""
			d.notice = "已复制"
			if onApplied != nil {
				onApplied()
			}
		}
	}) {
		l.stopBusy()
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
		}
		content := d.actionResult.text
		if content == "" {
			content = d.tr("结果已保存到历史。")
		}
		cardMaxHeight := float32(220)
		scrollMaxHeight := float32(196)
		if d.actionResult.qr != nil {
			cardMaxHeight = 120
			scrollMaxHeight = 96
		}
		ui.Box(c.Key("result-card")).Padding(12, 14).Radius(10).Background(k.DrawerInnerSurface()).Border(1, border).MaxHeight(cardMaxHeight).Children(func() {
			scroll := k.Scroll(c.Key("result-scroll")).MaxHeight(scrollMaxHeight)
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
		if d.actionResult.qr != nil {
			ui.Column(c.Key("qr-card")).Padding(14, 16).Radius(10).Background(k.DrawerInnerSurface()).Border(1, border).Gap(10).AlignItems(ui.Center).Children(func() {
				ui.Box(c.Key("qr-frame")).Padding(6).Radius(10).Background(ui.RGB(255, 255, 255)).Border(1, border).Children(func() {
					ui.Image(c.Key("qr-image"), d.actionResult.qr).Size(168, 168).Fit(ui.Contain).Role(ui.RoleImage).Label(d.tr("链接二维码"))
				})
				k.Text(c.Key("qr-hint"), d.tr("使用手机扫码打开链接"), desktopkit.TextProps{
					Role: desktopkit.RoleCaption,
					Tone: desktopkit.ToneSecondary,
				})
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
						d.toastError(d.tr("无法打开链接。"))
					}
				}
			}
		})
	})
	if !open {
		d.actionResult = actionResultState{}
	}
}
