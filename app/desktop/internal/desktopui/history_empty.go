package desktopui

import (
	"context"
	"fmt"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/appicons"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

var emptyBrandLogoSVG = sync.OnceValue(func() *ui.SVG {
	return ui.MustParseSVG(appicons.LogoSVG)
})

type emptyPalette struct {
	workspace       ui.Color
	card            ui.Color
	cardSubtle      ui.Color
	cardSubtleHover ui.Color
	sep             ui.Color
	cardBorder      ui.Color
	btnBorder       ui.Color
	btnBorderHover  ui.Color
	tertiaryBg      ui.Color
	ghostBg         ui.Color
	ghostBorder     ui.Color
	frontBg         ui.Color
	frontBorder     ui.Color
	frontShadow     ui.Color
	guideIconBg     ui.Color
	guideIconBorder ui.Color
	textPrimary     ui.Color
	textSecondary   ui.Color
	textTertiary    ui.Color
	okDot           ui.Color
	warnBg          ui.Color
	warnBorder      ui.Color
	warnFg          ui.Color
	warnDot         ui.Color
	accentBg        ui.Color
	accentBorder    ui.Color
	accentFg        ui.Color
}

func emptyPaletteFor(k desktopkit.Kit) emptyPalette {
	if k.Dark {
		return emptyPalette{
			workspace:       k.WorkspaceSurface(),
			card:            ui.RGB(36, 36, 40),
			cardSubtle:      ui.RGB(35, 35, 39),
			cardSubtleHover: ui.RGB(43, 43, 49),
			sep:             ui.RGBA(255, 255, 255, .08),
			cardBorder:      ui.RGBA(255, 255, 255, .095),
			btnBorder:       ui.RGBA(255, 255, 255, .14),
			btnBorderHover:  ui.RGBA(255, 255, 255, .24),
			tertiaryBg:      ui.RGB(44, 44, 50),
			ghostBg:         ui.RGB(37, 37, 43),
			ghostBorder:     ui.RGBA(255, 255, 255, .11),
			frontBg:         ui.RGB(43, 43, 50),
			frontBorder:     ui.RGBA(255, 255, 255, .14),
			frontShadow:     ui.RGBA(0, 0, 0, .38),
			guideIconBg:     ui.RGB(44, 44, 51),
			guideIconBorder: ui.RGBA(255, 255, 255, .09),
			textPrimary:     ui.RGB(244, 244, 246),
			textSecondary:   ui.RGB(156, 156, 166),
			textTertiary:    ui.RGB(110, 110, 120),
			okDot:           ui.RGB(132, 204, 22),
			warnBg:          ui.RGBA(217, 119, 6, .14),
			warnBorder:      ui.RGBA(245, 158, 11, .28),
			warnFg:          ui.RGB(251, 191, 36),
			warnDot:         ui.RGB(245, 158, 11),
			accentBg:        ui.RGBA(43, 127, 255, .14),
			accentBorder:    ui.RGBA(43, 127, 255, .24),
			accentFg:        ui.RGB(77, 160, 255),
		}
	}
	return emptyPalette{
		workspace:       k.WorkspaceSurface(),
		card:            ui.RGB(255, 255, 255),
		cardSubtle:      ui.RGB(247, 247, 249),
		cardSubtleHover: ui.RGB(240, 240, 244),
		sep:             ui.RGB(232, 232, 236),
		cardBorder:      ui.RGB(228, 228, 232),
		btnBorder:       ui.RGB(220, 220, 226),
		btnBorderHover:  ui.RGB(197, 197, 206),
		tertiaryBg:      ui.RGB(236, 236, 240),
		ghostBg:         ui.RGB(247, 247, 249),
		ghostBorder:     ui.RGB(228, 228, 232),
		frontBg:         ui.RGB(255, 255, 255),
		frontBorder:     ui.RGB(228, 228, 232),
		frontShadow:     ui.RGBA(17, 17, 20, .06),
		guideIconBg:     ui.RGB(255, 255, 255),
		guideIconBorder: ui.RGB(232, 232, 236),
		textPrimary:     ui.RGB(17, 17, 20),
		textSecondary:   ui.RGB(104, 104, 112),
		textTertiary:    ui.RGB(150, 150, 159),
		okDot:           ui.RGB(94, 165, 0),
		warnBg:          ui.RGB(255, 251, 235),
		warnBorder:      ui.RGB(253, 230, 138),
		warnFg:          ui.RGB(146, 64, 14),
		warnDot:         ui.RGB(217, 119, 6),
		accentBg:        ui.RGB(240, 246, 255),
		accentBorder:    ui.RGB(219, 234, 254),
		accentFg:        ui.RGB(21, 93, 252),
	}
}

func allHistoryQuery() engine.HistorySearchQuery {
	return engine.HistorySearchQuery{TagMatch: "all"}
}

func (d *viewState) historyHeadingTitle() string {
	switch {
	case d.nav.Value == "pinned":
		return d.tr("已收藏")
	case strings.HasPrefix(d.nav.Value, "tag:"):
		return d.tagLabel(strings.TrimPrefix(d.nav.Value, "tag:"))
	case strings.HasPrefix(d.nav.Value, "device:"):
		return d.sourceName(strings.TrimPrefix(d.nav.Value, "device:"))
	default:
		return d.tr("全部")
	}
}

func (d *viewState) hasExtraTagFilter() bool {
	tags := selectedSearchTags(d.tag)
	if len(tags) == 0 {
		return false
	}
	if strings.HasPrefix(d.nav.Value, "tag:") && len(tags) == 1 && tags[0] == strings.TrimPrefix(d.nav.Value, "tag:") {
		return false
	}
	return true
}

func (d *viewState) hasExtraSourceFilter() bool {
	if d.source == "" || d.source == "all" {
		return false
	}
	if strings.HasPrefix(d.nav.Value, "device:") && d.source == strings.TrimPrefix(d.nav.Value, "device:") {
		return false
	}
	return true
}

func (d *viewState) hasActiveSearchOrFilter() bool {
	return strings.TrimSpace(d.query) != "" ||
		(d.kind != "" && d.kind != "all") ||
		(d.timeRange != "" && d.timeRange != "all") ||
		d.search.extension != "" ||
		d.hasExtraTagFilter() ||
		d.hasExtraSourceFilter()
}

func (d *viewState) isSearchUnavailableState() bool {
	if d.unsupportedSearch() {
		return true
	}
	return d.live != nil && d.live.readError != "" && d.hasActiveSearchOrFilter()
}

func (d *viewState) totalHistoryCount() (int, bool) {
	if d.live == nil {
		return len(d.entries), true
	}
	if count, ok := d.live.counts[searchQueryKey(allHistoryQuery())]; ok {
		return int(count), true
	}
	if d.nav.Value == "all" && !d.hasActiveSearchOrFilter() && d.live.loaded {
		return len(d.entries), true
	}
	return 0, false
}

func (d *viewState) clearAllHistoryFilters() {
	d.search.clear(d)
	d.nav.Value = "all"
	d.filterOpen = false
	if d.live != nil {
		d.live.readError = ""
	}
}

type historyRelaxation struct {
	id    string
	label string
	query engine.HistorySearchQuery
	count int
	apply func()
}

func (d *viewState) buildRelaxationCandidates() []historyRelaxation {
	var out []historyRelaxation

	if d.nav.Value != "all" {
		scopeName := d.tr("已收藏")
		if strings.HasPrefix(d.nav.Value, "tag:") {
			scopeName = fmt.Sprintf(d.tr("标签「%s」"), d.tagLabel(strings.TrimPrefix(d.nav.Value, "tag:")))
		} else if strings.HasPrefix(d.nav.Value, "device:") {
			scopeName = fmt.Sprintf(d.tr("设备「%s」"), d.sourceName(strings.TrimPrefix(d.nav.Value, "device:")))
		}
		trial := *d
		if strings.HasPrefix(trial.nav.Value, "tag:") {
			navTag := strings.TrimPrefix(trial.nav.Value, "tag:")
			var kept []string
			for _, t := range selectedSearchTags(trial.tag) {
				if t != navTag {
					kept = append(kept, t)
				}
			}
			trial.tag = strings.Join(kept, "\x1f")
		} else if strings.HasPrefix(trial.nav.Value, "device:") && trial.source == strings.TrimPrefix(trial.nav.Value, "device:") {
			trial.source = ""
		}
		trial.nav.Value = "all"
		out = append(out, historyRelaxation{
			id:    "scope",
			label: fmt.Sprintf(d.tr("在「全部项目」中查找（退出%s）"), scopeName),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				if strings.HasPrefix(d.nav.Value, "tag:") {
					navTag := strings.TrimPrefix(d.nav.Value, "tag:")
					var kept []string
					for _, t := range selectedSearchTags(d.tag) {
						if t != navTag {
							kept = append(kept, t)
						}
					}
					d.tag = strings.Join(kept, "\x1f")
				} else if strings.HasPrefix(d.nav.Value, "device:") && d.source == strings.TrimPrefix(d.nav.Value, "device:") {
					d.source = ""
				}
				d.nav.Value = "all"
				d.search.refresh(d)
			},
		})
	}

	if d.timeRange != "" && d.timeRange != "all" {
		timeLabel := d.timeRangeLabel(d.timeRange)
		trial := *d
		trial.timeRange = ""
		out = append(out, historyRelaxation{
			id:    "time",
			label: fmt.Sprintf(d.tr("移除时间限制「%s」"), timeLabel),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				d.timeRange = ""
				d.search.refresh(d)
			},
		})
	}

	if d.kind != "" && d.kind != "all" {
		kindLabel := d.tr(d.kind)
		trial := *d
		trial.kind = ""
		out = append(out, historyRelaxation{
			id:    "type",
			label: fmt.Sprintf(d.tr("不限内容类型（移除「%s」）"), kindLabel),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				d.kind = ""
				d.search.refresh(d)
			},
		})
	}

	if d.hasExtraSourceFilter() {
		srcLabel := d.sourceName(d.source)
		trial := *d
		trial.source = ""
		out = append(out, historyRelaxation{
			id:    "source",
			label: fmt.Sprintf(d.tr("不限来源设备（移除「%s」）"), srcLabel),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				d.source = ""
				d.search.refresh(d)
			},
		})
	}

	if d.hasExtraTagFilter() {
		tags := selectedSearchTags(d.tag)
		navTag := ""
		if strings.HasPrefix(d.nav.Value, "tag:") {
			navTag = strings.TrimPrefix(d.nav.Value, "tag:")
		}
		var names []string
		for _, id := range tags {
			if id != navTag {
				names = append(names, d.tagLabel(id))
			}
		}
		tagLabel := strings.Join(names, ", ")
		trial := *d
		trial.tag = navTag
		out = append(out, historyRelaxation{
			id:    "tag",
			label: fmt.Sprintf(d.tr("移除标签「%s」"), tagLabel),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				d.tag = navTag
				d.search.refresh(d)
			},
		})
	}

	if d.search.extension != "" {
		ext := "." + d.search.extension
		trial := *d
		trial.search.extension = ""
		out = append(out, historyRelaxation{
			id:    "extension",
			label: fmt.Sprintf(d.tr("移除扩展名「%s」"), ext),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				d.search.extension = ""
				d.search.refresh(d)
			},
		})
	}

	qTrim := strings.TrimSpace(d.query)
	if qTrim != "" && (d.kind != "" || d.timeRange != "" || d.search.extension != "" || d.hasExtraSourceFilter() || d.hasExtraTagFilter()) {
		trial := *d
		trial.query = ""
		trial.search.buffer = ""
		out = append(out, historyRelaxation{
			id:    "query",
			label: fmt.Sprintf(d.tr("清除关键词 “%s”，仅保留筛选条件"), truncRunes(qTrim, 16)),
			query: trial.historySearchQuery(),
			count: d.candidateMatchCount(trial),
			apply: func() {
				d.query = ""
				d.search.buffer = ""
				d.search.last = ""
				d.search.start, d.search.end = 0, 0
				d.search.refresh(d)
			},
		})
	}

	return out
}

func (d *viewState) candidateMatchCount(trial viewState) int {
	if d.live == nil {
		return len(trial.filtered())
	}
	if n, ok := d.live.counts[searchQueryKey(trial.historySearchQuery())]; ok {
		return int(n)
	}
	return 0
}

func (d *viewState) searchRelaxationQueries() []engine.HistorySearchQuery {
	if !d.hasActiveSearchOrFilter() || d.unsupportedSearch() {
		return nil
	}
	cands := d.buildRelaxationCandidates()
	queries := make([]engine.HistorySearchQuery, 0, len(cands))
	for _, r := range cands {
		queries = append(queries, r.query)
	}
	return queries
}

func (d *viewState) activeRelaxations() []historyRelaxation {
	cands := d.buildRelaxationCandidates()
	var out []historyRelaxation
	for _, r := range cands {
		if r.count > 0 {
			out = append(out, r)
			if len(out) >= 4 {
				break
			}
		}
	}
	return out
}

func (d *viewState) timeRangeLabel(v string) string {
	for _, c := range d.searchValues() {
		if c.dimension == "time" && c.value == v {
			return d.tr(c.label)
		}
	}
	return v
}

func truncRunes(s string, maxLen int) string {
	r := []rune(s)
	if len(r) <= maxLen {
		return s
	}
	return string(r[:maxLen-1]) + "…"
}

func shortcutText(mac, other string) string {
	if runtime.GOOS == "darwin" {
		return mac
	}
	return other
}

func (d *viewState) trySampleCopy() {
	sampleText := d.tr("让每一次复制更有用 — UniClipboard 示例文本")
	if d.live == nil {
		id := fmt.Sprintf("sample-%d", time.Now().UnixNano())
		tags := []string{"builtin:text"}
		if strings.HasPrefix(d.nav.Value, "tag:") {
			tid := strings.TrimPrefix(d.nav.Value, "tag:")
			if tid != "builtin:text" {
				tags = append(tags, tid)
			}
		}
		src := "local"
		if strings.HasPrefix(d.nav.Value, "device:") {
			src = strings.TrimPrefix(d.nav.Value, "device:")
		}
		item := entry{
			id:     id,
			kind:   desktopkit.KindText,
			title:  sampleText,
			text:   sampleText,
			source: src,
			date:   d.tr("今天") + " " + time.Now().Format("15:04"),
			size:   fmt.Sprintf("%d", len([]rune(sampleText))),
			tags:   tags,
			pinned: d.nav.Value == "pinned",
		}
		d.entries = append([]entry{item}, d.entries...)
		d.clearAllHistoryFilters()
		d.selectEntry(id)
		d.notice = "已捕获新剪贴板内容"
		return
	}
	l := d.live
	if l.busy {
		return
	}
	l.busy = true
	request := newRequestID()
	snap := clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(sampleText)}}}
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 5*time.Second)
		defer cancel()
		out, err := l.client.Submit(ctx, api.SubmitInput{RequestID: request, Snapshot: snap})
		select {
		case l.results <- func() {
			l.busy = false
			if err != nil {
				l.message = "无法创建示例内容，请稍后重试。"
				return
			}
			d.clearAllHistoryFilters()
			if out.HistoryID != "" {
				l.pendingSelectID = string(out.HistoryID)
			}
			l.historyInvalid = true
			l.nextRefresh = time.Time{}
			d.notice = "已捕获新剪贴板内容"
		}:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

// historyEmptyState renders the unified full-canvas empty state for both Empty
// History (fresh, cleared, favorites, tag, device) and Search/Filter No-Results.
func (d *viewState) historyEmptyState(c *ui.Context, k desktopkit.Kit) {
	p := emptyPaletteFor(k)
	scroll := k.Scroll(c.Key("empty-scroll")).Grow(1).MinWidth(0).MinHeight(0)
	scroll.Children(func() {
		canvas := ui.Column(c.Key("empty-canvas")).Grow(1).FillWidth().MinHeight(420).
			Padding(24, 28, 40).AlignItems(ui.Center).Justify(ui.Center)
		canvas.Children(func() {
			shell := ui.Column(c.Key("empty-shell")).FillWidth().MaxWidth(448).
				AlignItems(ui.Center).Margin(0, 0, 12, 0)
			shell.Children(func() {
				switch {
				case d.isSearchUnavailableState():
					d.renderEmptySearchUnavailable(c, k, p)
				case d.hasActiveSearchOrFilter():
					d.renderEmptySearchNoResults(c, k, p)
				case d.nav.Value == "pinned":
					d.renderEmptyPinned(c, k, p)
				case strings.HasPrefix(d.nav.Value, "tag:"):
					d.renderEmptyTag(c, k, p, strings.TrimPrefix(d.nav.Value, "tag:"))
				case strings.HasPrefix(d.nav.Value, "device:"):
					d.renderEmptyDevice(c, k, p, strings.TrimPrefix(d.nav.Value, "device:"))
				default:
					d.renderEmptyFreshHistory(c, k, p)
				}
			})
		})
	})
}

func (d *viewState) renderEmptySearchUnavailable(c *ui.Context, k desktopkit.Kit, p emptyPalette) {
	emptySquircleBadge(c.Key("badge"), k, p, desktopkit.IconInfo, p.warnBg, p.warnBorder, p.warnFg, nil)
	title := d.tr("搜索索引暂不可用")
	desc := d.tr("本地搜索索引正在修复或遇到预算限制。为避免漏检，当前暂停关键词检索；你仍可按侧栏或时间浏览完整历史。")
	if d.unsupportedSearch() {
		title = d.tr("此搜索条件暂不可用")
		desc = d.tr("请选择类型、设备、标签或时间筛选。")
	} else if d.live != nil && d.live.readError != "" && !strings.Contains(d.live.readError, "索引") {
		desc = d.tr(d.live.readError)
	}
	emptyTitleText(c.Key("title"), title, p)
	emptyDescText(c.Key("desc"), desc, p)
	ui.Row(c.Key("actions")).Margin(18, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		if emptyActionButton(c.Key("browse-all"), k, p, d.tr("浏览全部历史"), "", "", true, false).Clicked() {
			d.clearAllHistoryFilters()
		}
		if d.live != nil && !d.unsupportedSearch() {
			if emptyActionButton(c.Key("retry"), k, p, d.tr("重试搜索"), "", "", false, false).Clicked() {
				d.live.readError = ""
				d.live.historyInvalid = true
				d.live.nextRefresh = time.Time{}
			}
		}
	})
}

func (d *viewState) renderEmptyFreshHistory(c *ui.Context, k desktopkit.Kit, p emptyPalette) {
	// Layered canvas emblem matching prototype .canvas-emblem (112×74).
	emblem := ui.Box(c.Key("canvas-emblem")).Size(112, 74).Margin(0, 0, 18, 0).Shrink(0).AccessHidden(true)
	emblem.Children(func() {
		ui.Box(c.Key("ghost-l")).Absolute().Left(4).Top(14).Size(68, 46).Radius(10).
			Background(p.ghostBg).Border(1, p.ghostBorder).Rotate(-7).Opacity(.78)
		ui.Box(c.Key("ghost-r")).Absolute().Right(4).Top(16).Size(68, 46).Radius(10).
			Background(p.ghostBg).Border(1, p.ghostBorder).Rotate(7).Opacity(.78)
		front := ui.Column(c.Key("front")).Absolute().Left(17).Top(4).Size(78, 58).Radius(12).
			Background(p.frontBg).Border(1, p.frontBorder).Shadow(0, 6, 18, 0, p.frontShadow).
			AlignItems(ui.Center).Justify(ui.Center).Gap(6)
		front.Children(func() {
			ui.Box(c.Key("cat")).Size(24, 24).Radius(6).Clip().Shrink(0).
				Background(ui.RGB(17, 17, 20)).Children(func() {
				ui.Image(c, emptyBrandLogoSVG()).Size(24, 24).Fit(ui.Contain).AccessHidden(true)
			})
			lineCol := p.tertiaryBg
			if k.Dark {
				lineCol = ui.RGBA(255, 255, 255, .14)
			}
			ui.Column(c.Key("lines")).Width(42).Gap(3.5).AlignItems(ui.Center).Children(func() {
				ui.Box(c.Key("l1")).Size(42, 3).Radius(1.5).Background(lineCol)
				ui.Box(c.Key("l2")).Size(27, 3).Radius(1.5).Background(lineCol)
			})
		})
	})

	// Live listening status pill (.listen-pill).
	connected := d.live == nil || d.live.connected
	pillDot := p.okDot
	pillText := d.tr("正在监听本机剪贴板")
	if !connected {
		pillDot = p.textTertiary
		pillText = d.tr("正在连接本地服务…")
	}
	ui.Row(c.Key("listen-pill")).Height(24).Padding(0, 10).Radius(12).Gap(6).Margin(0, 0, 12, 0).
		AlignItems(ui.Center).Background(p.cardSubtle).Border(1, p.cardBorder).Children(func() {
		ui.Box(c.Key("dot-halo")).Size(11, 11).Radius(5.5).Background(pillDot.Alpha(.20)).Center().Shrink(0).AccessHidden(true).Children(func() {
			ui.Box(c.Key("dot")).Size(7, 7).Radius(3.5).Background(pillDot)
		})
		ui.Text(c.Key("lbl"), pillText).FontSize(11.5).FontWeight(500).TextColor(p.textSecondary).SingleLine()
	})

	emptyTitleText(c.Key("title"), d.tr("还没有剪贴板历史"), p)
	emptyDescText(c.Key("desc"), d.tr("现在复制任意文字、代码、图片或文件，这里就会自动记录。"), p)

	// 3-Row BoardUI Inset Capability Card (.inset-card).
	remoteCount, onlineCount := 0, 0
	for _, dev := range d.devices {
		if dev.id == "local" {
			continue
		}
		remoteCount++
		if dev.online {
			onlineCount++
		}
	}
	hasDevices := remoteCount > 0

	ui.Column(c.Key("guide-card")).FillWidth().MaxWidth(424).Margin(20, 0, 0).Radius(12).Clip().
		Background(p.cardSubtle).Border(1, p.cardBorder).AlignItems(ui.Stretch).Children(func() {
		emptyGuideStaticRow(c.Key("row-copy"), k, p, desktopkit.IconCopy,
			d.tr("复制即刻整理"), d.tr("自动归类文本、图片与文件夹，仅保存在本机"),
			shortcutText("⌘C", "Ctrl+C"))
		ui.Box(c.Key("sep-1")).FillWidth().Height(1).Background(p.sep).Shrink(0)
		emptyGuideStaticRow(c.Key("row-quick"), k, p, desktopkit.IconSearch,
			d.tr("随时唤起快捷面板"), d.tr("无需切出当前应用，搜索后直接粘贴"),
			shortcutText("⇧⌘V", "Ctrl+Shift+V"))
		ui.Box(c.Key("sep-2")).FillWidth().Height(1).Background(p.sep).Shrink(0)

		devTitle := d.tr("关联其他设备")
		devSub := d.tr("与另一台电脑或手机配对，端到端加密同步剪贴板")
		devTrailing := d.tr("去关联")
		if hasDevices {
			devTitle = d.tr("跨设备同步已就绪")
			devSub = fmt.Sprintf(d.tr("已关联 %d 台设备（%d 台在线），复制后自动同步"), remoteCount, onlineCount)
			devTrailing = d.tr("管理")
		}
		btn := ui.ButtonBase(c.Key("row-devices")).FillWidth().Padding(11, 14).Gap(12).
			AlignItems(ui.Center).Label(devTitle).Role(ui.RoleButton).
			FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(-2)
		if btn.Hovered() {
			btn.Background(p.cardSubtleHover)
		}
		btn.Children(func() {
			tileBg, tileBorder, iconCol := p.guideIconBg, p.guideIconBorder, p.textSecondary
			if !hasDevices {
				tileBg, tileBorder, iconCol = p.accentBg, p.accentBorder, p.accentFg
			}
			ui.Box(c.Key("ic")).Size(32, 32).Radius(8).Background(tileBg).Border(1, tileBorder).Center().Shrink(0).AccessHidden(true).Children(func() {
				k.Icon(c, desktopkit.IconDevices, 15).TextColor(iconCol)
			})
			ui.Column(c.Key("body")).Grow(1).MinWidth(0).Gap(2).Children(func() {
				ui.Text(c.Key("t"), devTitle).FontSize(12.5).FontWeight(600).TextColor(p.textPrimary).SingleLine()
				ui.Text(c.Key("s"), devSub).FontSize(11.5).TextColor(p.textSecondary)
			})
			ui.Row(c.Key("end")).Gap(4).AlignItems(ui.Center).Shrink(0).Children(func() {
				ui.Text(c.Key("act"), devTrailing).FontSize(11.5).TextColor(p.textTertiary).SingleLine()
				k.Icon(c.Key("chev"), desktopkit.IconChevronRight, 14).TextColor(p.textTertiary).AccessHidden(true)
			})
		})
		if btn.Clicked() {
			d.page = "devices"
		}
	})

	// Bottom actions (.empty-actions).
	ui.Row(c.Key("actions")).Margin(18, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		if emptyActionButton(c.Key("sample-copy"), k, p, d.tr("复制示例试试"), desktopkit.IconCopy, "", false, false).Clicked() {
			d.trySampleCopy()
		}
		if !hasDevices {
			if emptyActionButton(c.Key("import-v1"), k, p, d.tr("从旧版导入…"), "", "", false, true).Clicked() {
				if d.live != nil && d.live.openMigration != nil {
					d.live.openMigration()
				} else {
					d.page = "settings"
					d.settings.Value = "storage"
				}
			}
		}
	})
}

func (d *viewState) renderEmptyPinned(c *ui.Context, k desktopkit.Kit, p emptyPalette) {
	emptySquircleBadge(c.Key("badge"), k, p, desktopkit.IconPin, p.warnBg, p.warnBorder, p.warnDot, nil)
	emptyTitleText(c.Key("title"), d.tr("还没有收藏的内容"), p)
	emptyDescText(c.Key("desc"), fmt.Sprintf(d.tr("选中任意历史条目后点击工具栏星标，或按下 %s，即可把常用片段固定在这里。"), shortcutText("⌘P", "Ctrl+P")), p)
	total, known := d.totalHistoryCount()
	ui.Row(c.Key("actions")).Margin(18, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		if !known || total > 0 {
			lbl := d.tr("返回全部项目")
			if known && total > 0 {
				lbl = fmt.Sprintf(d.tr("返回全部项目 (%d)"), total)
			}
			if emptyActionButton(c.Key("go-all"), k, p, lbl, desktopkit.IconBack, "", false, false).Clicked() {
				d.nav.Value = "all"
				d.tag, d.source = "", ""
			}
		} else {
			if emptyActionButton(c.Key("sample-copy"), k, p, d.tr("复制示例试试"), desktopkit.IconCopy, "", false, false).Clicked() {
				d.trySampleCopy()
			}
		}
	})
}

func (d *viewState) renderEmptyTag(c *ui.Context, k desktopkit.Kit, p emptyPalette, tagID string) {
	label := d.tagLabel(tagID)
	var col desktopkit.TagColor
	if t := d.tagByID(tagID); t != nil {
		col = t.color
	}
	bg, fg := k.TagTint(col)
	swatch := k.SwatchColor(col)
	emptySquircleBadge(c.Key("badge"), k, p, desktopkit.IconTag, bg, swatch.Alpha(.28), fg, &swatch)
	emptyTitleText(c.Key("title"), fmt.Sprintf(d.tr("“%s” 标签下还没有内容"), label), p)
	emptyDescText(c.Key("desc"), fmt.Sprintf(d.tr("选中任意历史条目后按 T 添加「%s」标签，相关内容会自动归集在这里。"), label), p)
	total, known := d.totalHistoryCount()
	ui.Row(c.Key("actions")).Margin(18, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		if !known || total > 0 {
			lbl := d.tr("返回全部项目")
			if known && total > 0 {
				lbl = fmt.Sprintf(d.tr("返回全部项目 (%d)"), total)
			}
			if emptyActionButton(c.Key("go-all"), k, p, lbl, desktopkit.IconBack, "", false, false).Clicked() {
				d.nav.Value = "all"
				d.tag, d.source = "", ""
			}
		}
		if emptyActionButton(c.Key("manage-tags"), k, p, d.tr("管理标签…"), "", "", false, true).Clicked() {
			d.tagsOpen = true
		}
	})
}

func (d *viewState) renderEmptyDevice(c *ui.Context, k desktopkit.Kit, p emptyPalette, devID string) {
	name := d.sourceName(devID)
	online := false
	for _, dev := range d.devices {
		if dev.id == devID {
			online = dev.online
			break
		}
	}
	dot := p.textTertiary
	if online {
		dot = p.okDot
	}
	emptySquircleBadge(c.Key("badge"), k, p, desktopkit.IconDesktop, p.cardSubtle, p.cardBorder, p.textSecondary, &dot)
	emptyTitleText(c.Key("title"), fmt.Sprintf(d.tr("还没有来自 %s 的历史"), name), p)
	desc := fmt.Sprintf(d.tr("%s 当前离线。设备重新连线后，新复制的内容会自动同步到这里。"), name)
	if online {
		desc = fmt.Sprintf(d.tr("在 %s 上复制内容后，会自动同步到这台设备。"), name)
	}
	emptyDescText(c.Key("desc"), desc, p)
	total, known := d.totalHistoryCount()
	ui.Row(c.Key("actions")).Margin(18, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		if !known || total > 0 {
			lbl := d.tr("返回全部项目")
			if known && total > 0 {
				lbl = fmt.Sprintf(d.tr("返回全部项目 (%d)"), total)
			}
			if emptyActionButton(c.Key("go-all"), k, p, lbl, desktopkit.IconBack, "", false, false).Clicked() {
				d.nav.Value = "all"
				d.tag, d.source = "", ""
			}
		}
		if emptyActionButton(c.Key("device-status"), k, p, d.tr("查看设备状态…"), "", "", false, true).Clicked() {
			d.page = "devices"
		}
	})
}

type emptyConstraintChip struct {
	id, field, value string
	kind             desktopkit.SearchTokenKind
	remove           func()
}

func (d *viewState) activeConstraintChips() []emptyConstraintChip {
	var chips []emptyConstraintChip
	switch {
	case d.nav.Value == "pinned":
		chips = append(chips, emptyConstraintChip{
			id: "scope", field: d.tr("范围"), value: d.tr("已收藏"), kind: desktopkit.SearchKindOther,
			remove: func() { d.nav.Value = "all" },
		})
	case strings.HasPrefix(d.nav.Value, "tag:"):
		tid := strings.TrimPrefix(d.nav.Value, "tag:")
		chips = append(chips, emptyConstraintChip{
			id: "scope", field: d.tr("范围"), value: d.tagLabel(tid), kind: desktopkit.SearchKindTag,
			remove: func() {
				var kept []string
				for _, t := range selectedSearchTags(d.tag) {
					if t != tid {
						kept = append(kept, t)
					}
				}
				d.tag = strings.Join(kept, "\x1f")
				d.nav.Value = "all"
			},
		})
	case strings.HasPrefix(d.nav.Value, "device:"):
		did := strings.TrimPrefix(d.nav.Value, "device:")
		chips = append(chips, emptyConstraintChip{
			id: "scope", field: d.tr("范围"), value: d.sourceName(did), kind: desktopkit.SearchKindSource,
			remove: func() {
				if d.source == did {
					d.source = ""
				}
				d.nav.Value = "all"
			},
		})
	}
	if d.kind != "" && d.kind != "all" {
		chips = append(chips, emptyConstraintChip{
			id: "kind", field: d.tr("类型"), value: d.tr(d.kind), kind: desktopkit.SearchKindType,
			remove: func() { d.kind = ""; d.search.refresh(d) },
		})
	}
	if d.hasExtraTagFilter() {
		navTag := ""
		if strings.HasPrefix(d.nav.Value, "tag:") {
			navTag = strings.TrimPrefix(d.nav.Value, "tag:")
		}
		var names []string
		for _, id := range selectedSearchTags(d.tag) {
			if id != navTag {
				names = append(names, d.tagLabel(id))
			}
		}
		chips = append(chips, emptyConstraintChip{
			id: "tag", field: d.tr("标签"), value: strings.Join(names, ", "), kind: desktopkit.SearchKindTag,
			remove: func() { d.tag = navTag; d.search.refresh(d) },
		})
	}
	if d.hasExtraSourceFilter() {
		chips = append(chips, emptyConstraintChip{
			id: "source", field: d.tr("设备"), value: d.sourceName(d.source), kind: desktopkit.SearchKindSource,
			remove: func() { d.source = ""; d.search.refresh(d) },
		})
	}
	if d.timeRange != "" && d.timeRange != "all" {
		chips = append(chips, emptyConstraintChip{
			id: "time", field: d.tr("时间"), value: d.timeRangeLabel(d.timeRange), kind: desktopkit.SearchKindTime,
			remove: func() { d.timeRange = ""; d.search.refresh(d) },
		})
	}
	if d.search.extension != "" {
		chips = append(chips, emptyConstraintChip{
			id: "ext", field: d.tr("扩展名"), value: "." + d.search.extension, kind: desktopkit.SearchKindOther,
			remove: func() { d.search.extension = ""; d.search.refresh(d) },
		})
	}
	return chips
}

func (d *viewState) renderEmptySearchNoResults(c *ui.Context, k desktopkit.Kit, p emptyPalette) {
	qText := strings.TrimSpace(d.query)
	chips := d.activeConstraintChips()
	relaxations := d.activeRelaxations()
	isFilterOnly := qText == "" && len(chips) > 0

	badgeIcon := desktopkit.IconSearch
	if isFilterOnly {
		badgeIcon = desktopkit.IconFilter
	}
	emptySquircleBadge(c.Key("badge"), k, p, badgeIcon, p.cardSubtle, p.cardBorder, p.textSecondary, nil)

	// Title + Description.
	if isFilterOnly {
		emptyTitleText(c.Key("title"), d.tr("当前筛选条件下没有内容"), p)
		emptyDescText(c.Key("desc"), d.tr("同时满足以下条件的历史条目为空，可以移除部分条件或扩大时间范围。"), p)
	} else {
		shown := truncRunes(qText, 28)
		tmpl := d.tr("没有找到匹配 %s 的内容")
		fullTitle := fmt.Sprintf(tmpl, shown)
		pre, suf, hasSplit := strings.Cut(tmpl, "%s")
		pre, suf = strings.TrimSpace(pre), strings.TrimSpace(suf)
		ui.Row(c.Key("title-row")).Wrap().Justify(ui.Center).AlignItems(ui.Center).Gap(5).MaxWidth(420).
			Role(ui.RoleHeading).Label(fullTitle).Children(func() {
			if hasSplit {
				if pre != "" {
					ui.Text(c.Key("pre"), pre).FontSize(17).FontWeight(600).TextColor(p.textPrimary)
				}
				ui.Box(c.Key("q-pill")).Padding(1, 7).Radius(6).Background(p.cardSubtle).Border(1, p.sep).Children(func() {
					ui.Text(c.Key("q"), shown).FontSize(16).FontWeight(600).TextColor(p.textPrimary).SingleLine()
				})
				if suf != "" {
					ui.Text(c.Key("suf"), suf).FontSize(17).FontWeight(600).TextColor(p.textPrimary)
				}
			} else {
				ui.Text(c.Key("full"), fullTitle).FontSize(17).FontWeight(600).TextColor(p.textPrimary).TextAlign(ui.Center)
			}
		})
		if len(chips) > 0 {
			emptyDescText(c.Key("desc"), fmt.Sprintf(d.tr("当前同时生效了 %d 项范围或筛选条件，放宽任意一项即可扩大查找范围。"), len(chips)), p)
		} else {
			emptyDescText(c.Key("desc"), d.tr("请检查拼写或换用更短的关键词。搜索匹配纯文本、网页正文与文件名，不检索图片文字或文件内部内容。"), p)
		}
	}

	// Active filter echo chips (.active-echo).
	if len(chips) > 0 {
		ui.Row(c.Key("echo")).Margin(14, 0, 0).Gap(6).Wrap().Justify(ui.Center).MaxWidth(424).Children(func() {
			for _, ch := range chips {
				chip := ch
				r := k.SearchToken(c.Key("echo-"+chip.id), desktopkit.SearchTokenProps{
					Field:       chip.field + ":",
					Value:       chip.value,
					Kind:        chip.kind,
					RemoveLabel: fmt.Sprintf(d.tr("清除%s筛选"), chip.field),
				})
				if r.Removed {
					chip.remove()
				}
			}
		})
	}

	// Smart Relaxation Card (.relax-card).
	var scopeRelax *historyRelaxation
	for i := range relaxations {
		if relaxations[i].id == "scope" {
			scopeRelax = &relaxations[i]
			break
		}
	}
	if len(relaxations) > 0 {
		ui.Column(c.Key("relax-card")).FillWidth().MaxWidth(424).Margin(18, 0, 0).Radius(12).Clip().
			Background(p.cardSubtle).Border(1, p.cardBorder).AlignItems(ui.Stretch).Children(func() {
			ui.Row(c.Key("hd")).Padding(8, 14, 6).AlignItems(ui.Center).Children(func() {
				ui.Text(c.Key("l"), d.tr("试试放宽条件")).FontSize(11).FontWeight(600).TextColor(p.textTertiary).Grow(1)
				ui.Text(c.Key("r"), d.tr("点击立即切换")).FontSize(11).TextColor(p.textTertiary).SingleLine()
			})
			for _, rel := range relaxations {
				r := rel
				ui.Box(c.Key("sep-"+r.id)).FillWidth().Height(1).Background(p.sep).Shrink(0)
				row := ui.ButtonBase(c.Key("rel-"+r.id)).FillWidth().Padding(9, 14).Gap(10).
					AlignItems(ui.Center).Label(r.label).Role(ui.RoleButton).
					FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(-2)
				hovered := row.Hovered()
				if hovered {
					row.Background(p.cardSubtleHover)
				}
				row.Children(func() {
					icCol, icEdge := p.textSecondary, p.sep
					if hovered {
						icCol, icEdge = p.accentFg, p.accentBorder
					}
					ui.Box(c.Key("ic")).Size(24, 24).Radius(6).Background(p.card).Border(1, icEdge).Center().Shrink(0).AccessHidden(true).Children(func() {
						k.Icon(c, desktopkit.IconRetry, 13).TextColor(icCol)
					})
					ui.Text(c.Key("lbl"), r.label).FontSize(12.5).FontWeight(500).TextColor(p.textPrimary).Grow(1).MinWidth(0)
					ui.Box(c.Key("cnt")).Padding(2, 8).Radius(999).Background(p.accentBg).Shrink(0).Children(func() {
						ui.Text(c, fmt.Sprintf(d.tr("%d 条"), r.count)).FontSize(11.5).FontWeight(600).TextColor(p.accentFg).SingleLine()
					})
					k.Icon(c.Key("chev"), desktopkit.IconChevronRight, 14).TextColor(p.textTertiary).AccessHidden(true)
				})
				if row.Clicked() {
					r.apply()
				}
			}
		})
	}

	// Action buttons (.empty-actions).
	ui.Row(c.Key("actions")).Margin(18, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		if scopeRelax != nil {
			sr := *scopeRelax
			if emptyActionButton(c.Key("expand-scope"), k, p, fmt.Sprintf(d.tr("在全部项目中搜索 (%d)"), sr.count), "", "", true, false).Clicked() {
				sr.apply()
			}
			if emptyActionButton(c.Key("clear-all"), k, p, d.tr("清除全部筛选"), "", "Esc", false, false).Clicked() {
				d.clearAllHistoryFilters()
			}
		} else {
			lbl := d.tr("清除全部筛选")
			if qText != "" && len(chips) == 0 {
				lbl = d.tr("清除搜索词")
			}
			if emptyActionButton(c.Key("clear-all"), k, p, lbl, "", "Esc", false, false).Clicked() {
				d.clearAllHistoryFilters()
			}
		}
	})

	// Syntax hint strip on keyword-only miss (.syntax-strip).
	if !isFilterOnly && len(chips) == 0 {
		ui.Row(c.Key("syntax")).Margin(20, 0, 0).Gap(6).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
			ui.Text(c.Key("lbl"), d.tr("按条件筛选：")).FontSize(11.5).TextColor(p.textSecondary)
			syntaxPill := func(key, text string, onClick func()) {
				b := ui.ButtonBase(c.Key(key)).Height(24).Padding(0, 8).Radius(6).
					Background(p.cardSubtle).Border(1, p.cardBorder).AlignItems(ui.Center).Justify(ui.Center).
					Label(text).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
				if b.Hovered() {
					b.Background(p.card).Border(1, p.btnBorderHover)
				}
				b.Children(func() {
					ui.Text(c, text).FontSize(11.5).TextColor(p.textPrimary).SingleLine()
				})
				if b.Clicked() {
					onClick()
				}
			}
			syntaxPill("syn-img", "/"+d.tr("图片"), func() {
				d.search.clear(d)
				d.setSearchCondition("type", "image")
			})
			syntaxPill("syn-folder", "/"+d.tr("文件夹"), func() {
				d.search.clear(d)
				d.setSearchCondition("type", "folder")
			})
			syntaxPill("syn-today", "on:"+d.tr("今天"), func() {
				d.search.clear(d)
				d.setSearchCondition("time", "today")
			})
			for _, dev := range d.devices {
				if dev.id == "local" {
					continue
				}
				did, dname := dev.id, dev.name
				syntaxPill("syn-dev", "@"+truncRunes(dname, 12), func() {
					d.search.clear(d)
					d.setSearchCondition("source", did)
				})
				break
			}
		})
	}
}

func emptySquircleBadge(c *ui.Context, k desktopkit.Kit, p emptyPalette, icon desktopkit.IconName, bg, border, fg ui.Color, pipColor *ui.Color) {
	wrap := ui.Box(c).Size(52, 52).Margin(0, 0, 16, 0).Shrink(0).AccessHidden(true)
	wrap.Children(func() {
		ui.Box(c.Key("tile")).Fill().Radius(15).Background(bg).Border(1, border).Center().Children(func() {
			k.Icon(c, icon, 24).TextColor(fg)
		})
		if pipColor != nil {
			ui.Box(c.Key("pip")).Absolute().Right(-3).Bottom(-3).Size(16, 16).Radius(8).
				Background(p.workspace).Center().Children(func() {
				ui.Box(c.Key("dot")).Size(9, 9).Radius(4.5).Background(*pipColor)
			})
		}
	})
}

func emptyTitleText(c *ui.Context, text string, p emptyPalette) {
	ui.Box(c).MaxWidth(420).Children(func() {
		ui.Text(c, text).Role(ui.RoleHeading).FontSize(17).FontWeight(600).
			TextColor(p.textPrimary).TextAlign(ui.Center)
	})
}

func emptyDescText(c *ui.Context, text string, p emptyPalette) {
	ui.Box(c).MaxWidth(444).Margin(7, 0, 0).Children(func() {
		ui.Text(c, text).FontSize(13).TextColor(p.textSecondary).TextAlign(ui.Center)
	})
}

func emptyGuideStaticRow(c *ui.Context, k desktopkit.Kit, p emptyPalette, icon desktopkit.IconName, title, sub, kbd string) {
	ui.Row(c).FillWidth().Padding(11, 14).Gap(12).AlignItems(ui.Center).Children(func() {
		ui.Box(c.Key("ic")).Size(32, 32).Radius(8).Background(p.guideIconBg).Border(1, p.guideIconBorder).
			Center().Shrink(0).AccessHidden(true).Children(func() {
			k.Icon(c, icon, 15).TextColor(p.textSecondary)
		})
		ui.Column(c.Key("body")).Grow(1).MinWidth(0).Gap(2).Children(func() {
			ui.Text(c.Key("t"), title).FontSize(12.5).FontWeight(600).TextColor(p.textPrimary).SingleLine()
			ui.Text(c.Key("s"), sub).FontSize(11.5).TextColor(p.textSecondary)
		})
		if kbd != "" {
			emptyKeycap(c.Key("kbd"), p, kbd)
		}
	})
}

func emptyKeycap(c *ui.Context, p emptyPalette, text string) {
	ui.Box(c).Padding(3, 6).Radius(5).Background(p.card).Border(1, p.btnBorder).
		Shadow(0, 1, 0, 0, p.sep).Shrink(0).AccessHidden(true).Children(func() {
		ui.Text(c, text).FontSize(11).FontWeight(500).TextColor(p.textPrimary).SingleLine()
	})
}

func emptyActionButton(c *ui.Context, k desktopkit.Kit, p emptyPalette, label string, icon desktopkit.IconName, kbd string, primary, subtle bool) ui.Element {
	b := ui.ButtonBase(c).Height(32).Padding(0, 14).Radius(8).Gap(6).
		AlignItems(ui.Center).Justify(ui.Center).Label(label).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
	fg := p.textPrimary
	switch {
	case primary:
		bg := k.Color("accent-500")
		if b.Hovered() {
			bg = k.Color("accent-600")
		}
		b.Background(bg).Border(1, bg)
		fg = k.Color("text-white")
	case subtle:
		fg = p.textSecondary
		if b.Hovered() {
			b.Background(p.cardSubtleHover)
			fg = p.textPrimary
		}
	default:
		bg, edge := p.card, p.btnBorder
		if b.Hovered() {
			bg, edge = p.cardSubtle, p.btnBorderHover
		}
		b.Background(bg).Border(1, edge)
		if !k.Dark {
			b.Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, .04))
		}
	}
	b.TextColor(fg)
	b.Children(func() {
		if icon != "" {
			k.Icon(c.Key("ic"), icon, 14).TextColor(fg).AccessHidden(true)
		}
		ui.Text(c.Key("lbl"), label).FontSize(12.5).FontWeight(500).TextColor(fg).SingleLine()
		if kbd != "" {
			kbdBg := p.tertiaryBg
			kbdFg := p.textSecondary
			if primary {
				kbdBg = ui.RGBA(255, 255, 255, .20)
				kbdFg = k.Color("text-white")
			}
			ui.Box(c.Key("kbd")).Padding(1, 5).Radius(4).Background(kbdBg).AccessHidden(true).Children(func() {
				ui.Text(c, kbd).FontSize(10.5).FontWeight(500).TextColor(kbdFg).SingleLine()
			})
		}
	})
	return b
}
