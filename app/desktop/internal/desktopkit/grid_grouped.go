package desktopkit

import (
	"math"
	"net/url"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
)

// gridSel 是 GroupedGrid 的专有状态（嵌在 ListState 里）。
type gridSel struct {
	cols   int
	width  float32
	anchor int      // Shift 扩选的起点（条目序号）
	rowOf  []int    // 条目序号 -> 可视行号
	hover  string   // 本帧被指针悬停的卡片（右键菜单作用对象）
	ids    []string // 条目序号 -> id
}

// GridTag is a compact colored tag pill rendered in a grid card footer.
type GridTag struct {
	Label string
	Color TagColor
}

type GridItem struct {
	ID       string
	Title    string
	Subtitle string
	// Kind 决定卡片主体画什么：图片（Media）、深色代码片段、链接卡、文件清单或文本。
	Kind ContentKind
	// KindLabel is the localized short label for the content kind (e.g. "文本", "代码", "链接").
	KindLabel string
	// Text 是主体里的文字（代码/链接/文件列表/文本）。
	Text string
	// Time is the short timestamp shown at the trailing edge of the A1 card header.
	Time string
	// Meta is the compact footer text (e.g. "工作电脑 · 96 个字符"); falls back to Subtitle.
	Meta string
	// SourceAppName and SourceAppIcon identify the originating application in the A1 header.
	SourceAppName string
	SourceAppIcon ui.ImageSource
	// Pinned marks pinned entries with a subtle pin indicator.
	Pinned bool
	// Tags holds compact colored tag pills rendered in the card footer.
	Tags []GridTag
	// Media 构建图片主体（如缩略图元素）。
	Media func()
	// Badges 画在卡片上的额外小标记。
	Badges func()
	// OnCopy and OnPin handle hover quick actions without changing selection.
	OnCopy    func()
	OnPin     func()
	CopyLabel string
	PinLabel  string
}

type GroupedGridProps struct {
	Viewport *HistoryWindowViewport
	Label    string
	Groups   []ListGroup
	Items    []GridItem
	// Size 是工具栏滑块给出的卡片尺寸，0 最小到 1 最大。
	Size   float32
	Menu   func(ids []string) []MenuEntry
	Footer func()
}

type ggRow struct {
	header int // >=0：分组头（Groups 下标）
	first  int // 卡片行：首个条目序号；-1 为占位
	n      int
}

// GroupedGrid 是分组的语义卡片网格（方案 A1：顶部来源应用锚点 + 内容语义画布）。
func (k Kit) GroupedGrid(c *ui.Context, st *ListState, p GroupedGridProps) ui.Element {
	st.begin()
	g := &st.grid
	size := float32(math.Max(0, math.Min(1, float64(p.Size))))
	cardMin := 148 + 96*size
	gap := float32(12)
	width := g.width
	if width <= 0 {
		width = 640
	}
	cols := max(1, int(math.Floor(float64((width+gap)/(cardMin+gap)))))
	g.cols = cols
	cardH := 154 + 56*size
	if g.cols == 0 {
		g.cols = 1
	}

	off := 0
	if p.Viewport != nil {
		off = max(p.Viewport.Offset, 0)
	}
	var rows []ggRow
	holes := func(n int) {
		for n > 0 {
			rows = append(rows, ggRow{header: -1, first: -1, n: min(cols, n)})
			n -= cols
		}
	}
	if p.Viewport != nil {
		holes(off)
	}
	loLoaded := len(rows)
	g.rowOf = g.rowOf[:0]
	g.ids = g.ids[:0]
	for _, it := range p.Items {
		g.ids = append(g.ids, it.ID)
		g.rowOf = append(g.rowOf, 0)
	}
	next := 0
	cardRows := func(count int) {
		for count > 0 {
			n := min(cols, count)
			for j := 0; j < n; j++ {
				g.rowOf[next+j] = len(rows)
			}
			rows = append(rows, ggRow{header: -1, first: next, n: n})
			next += n
			count -= n
		}
	}
	for gi, grp := range p.Groups {
		n := min(grp.Count, len(p.Items)-next)
		if n <= 0 && grp.Count <= 0 {
			continue
		}
		rows = append(rows, ggRow{header: gi, first: -1})
		cardRows(max(n, 0))
	}
	cardRows(len(p.Items) - next)
	hiLoaded := len(rows)
	if p.Viewport != nil {
		holes(p.Viewport.Total - off - len(p.Items))
	} else {
		loLoaded, hiLoaded = 0, 0
	}

	keys := make([]string, len(rows))
	for i, r := range rows {
		switch {
		case r.header >= 0:
			keys[i] = "\x00h" + p.Groups[r.header].ID
		case r.first >= 0:
			keys[i] = "\x00c" + p.Items[r.first].ID
		default:
			keys[i] = "\x00hole" + itoaPos(i)
		}
	}
	st.ids = nil
	if st.rowOf == nil {
		st.rowOf = map[string]int{}
	}
	clear(st.rowOf)
	for i, it := range p.Items {
		st.rowOf[it.ID] = i
	}
	st.list.Selected, st.list.Selection = nil, nil
	st.list.Key = func(i int) any {
		if i >= 0 && i < len(keys) {
			return keys[i]
		}
		return i
	}
	st.list.Header = func(i int) bool { return i >= 0 && i < len(rows) && rows[i].header >= 0 }
	st.list.Label = nil
	reveal := p.Viewport != nil && p.Viewport.Reveal != "" && p.Viewport.Reveal != st.vpReveal
	st.vpCommands(p.Viewport)
	if reveal {
		if i, ok := st.rowOf[p.Viewport.Reveal]; ok {
			st.list.ScrollIntoView(g.rowOf[i])
		}
	}
	if st.cur >= len(p.Items) {
		st.cur = len(p.Items) - 1
	}
	g.hover = ""

	shell := ui.Column(c)
	shell.Children(func() {
		restore := k.withListTheme(c)
		const gridPadX = float32(16)
		e := scrollingIndicator(c, ui.List(c, &st.list, len(rows))).Rows(func(r ui.ListRow) {
			row := rows[r.Index]
			rc := r.Context
			switch {
			case row.header >= 0:
				k.gridGroupHeader(rc, p.Groups[row.header])
			default:
				k.gridCardRow(rc, st, p, row, cols, cardH, gap)
			}
		})
		restore()
		e.Label(p.Label).Grow(1).Padding(0, gridPadX, 0, gridPadX).Focusable()
		e.Multiselectable(true)
		g.width = max(e.Bounds().W-2*gridPadX, 0)
		k.gridKeys(e, st, p, cols)
		st.gridSettle(e)
		st.visibleGrid(len(rows), [2]int{loLoaded, hiLoaded}, rows, off)
		k.listContextMenu(c, e, st, p.Menu, g.hover)
		if p.Footer != nil {
			k.GroupedListFooter(c, p.Footer)
		}
	})
	return shell.Grow(1)
}

func itoaPos(i int) string {
	return string(rune('0'+i%10)) + func() string {
		if i >= 10 {
			return itoaPos(i / 10)
		}
		return ""
	}()
}

// gridGroupHeader aligns grid section titles flush with the 16-DIP card rail.
func (k Kit) gridGroupHeader(c *ui.Context, g ListGroup) {
	lk := k.listLook()
	h := ui.Row(c).AlignItems(ui.Center).Gap(8).Height(lk.headerH).Background(k.surfaceSolid())
	h.Role(ui.RoleNone)
	h.Children(func() {
		ui.Text(c, g.Title).FontSize(lk.titleSize - 1).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
		if g.Detail != "" {
			ui.Text(c, g.Detail).FontSize(lk.metaSize).TextColor(k.Color("text-secondary")).SingleLine()
		}
		ui.Spacer(c)
		if g.Trailing != "" {
			ui.Text(c, g.Trailing).FontSize(lk.metaSize).TextColor(k.Color("text-secondary")).SingleLine()
		}
	})
}

// gridCardRow 画一行卡片（cols 张，不足补空位使卡片等宽）。
func (k Kit) gridCardRow(c *ui.Context, st *ListState, p GroupedGridProps, row ggRow, cols int, cardH, gap float32) {
	r := ui.Row(c).Gap(gap).PaddingY(gap / 2).AlignItems(ui.Stretch).Role(ui.RoleNone)
	r.Children(func() {
		if row.first < 0 { // 占位行
			for j := 0; j < cols; j++ {
				ui.Box(c.Key(j)).Grow(1).Basis(0).MinWidth(0).Height(cardH)
			}
			return
		}
		for j := 0; j < row.n; j++ {
			i := row.first + j
			k.gridCard(c.Key(p.Items[i].ID), st, p.Items[i], i, cardH)
		}
		for j := row.n; j < cols; j++ {
			ui.Box(c.Key("pad" + itoaPos(j))).Grow(1).Basis(0).MinWidth(0)
		}
	})
}

func (k Kit) gridCard(c *ui.Context, st *ListState, it GridItem, i int, cardH float32) {
	selected := st.sel.Has(it.ID)
	rad := float32(12)

	surface := k.CardSurface()
	if selected {
		alpha := float32(.06)
		if k.Dark {
			alpha = .13
		}
		surface = k.Color("accent-500").Alpha(alpha).Over(surface)
	}
	card := ui.Column(c).Grow(1).Basis(0).MinWidth(0).Height(cardH).Radius(rad).Clip().
		Background(surface).Label(it.Title)
	card.Role(ui.RoleListItem)
	card.AccessSelected(selected)

	border := k.Color("separator-border")
	hovered := card.Hovered()
	if hovered {
		st.grid.hover = it.ID
		if !selected {
			card.Background(k.Color("background-primary-hover").MixOklab(surface, .55))
			border = k.Color("border-button-hover")
		}
	}
	card.Border(1, border)

	ring := k.Color("accent-500")
	card.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if selected {
			p.Stroke(rc, ring, rad, 1.8)
		}
	})

	actionConsumed := false
	card.Children(func() {
		if it.Kind == KindImage {
			k.gridImageBody(c, it)
		} else {
			k.gridA1Header(c, it, hovered && (it.OnCopy != nil || it.OnPin != nil))
			k.gridSemanticBody(c, it)
		}
		k.gridA1Footer(c, it)

		if it.Badges != nil {
			badges := ui.Row(c.Key("badges")).Absolute().Top(6).Right(6).Gap(4)
			badges.Children(it.Badges)
		}
		if hovered && (it.OnCopy != nil || it.OnPin != nil) {
			pillBg := k.Color("background-primary-default").Alpha(.95)
			qa := ui.Row(c.Key("qa")).Absolute().Top(7).Right(7).Padding(2).Gap(2).Radius(8).
				Background(pillBg).Border(1, k.Color("border-button-default")).ZIndex(4)
			k.shadowed(qa, "xs")
			qa.Children(func() {
				if it.OnCopy != nil {
					lbl := it.CopyLabel
					if lbl == "" {
						lbl = "Copy"
					}
					b := ui.ButtonBase(c.Key("copy")).Size(24, 24).Radius(6).Center().Label(lbl).Tooltip(lbl).TabStop(false)
					ic := k.Color("text-secondary")
					if b.Hovered() {
						b.Background(k.Color("background-secondary-hover"))
						ic = k.Color("text-primary")
					}
					b.Children(func() { k.Icon(c, IconCopy, 13).TextColor(ic) })
					if b.Clicked() {
						actionConsumed = true
						it.OnCopy()
					}
				}
				if it.OnPin != nil {
					lbl := it.PinLabel
					if lbl == "" {
						lbl = "Pin"
					}
					b := ui.ButtonBase(c.Key("pin")).Size(24, 24).Radius(6).Center().Label(lbl).Tooltip(lbl).TabStop(false)
					ic := k.Color("text-secondary")
					if it.Pinned {
						ic = k.Color("status-orange-text")
					}
					if b.Hovered() {
						b.Background(k.Color("background-secondary-hover"))
						if !it.Pinned {
							ic = k.Color("text-primary")
						}
					}
					b.Children(func() { k.Icon(c, IconPin, 13).TextColor(ic) })
					if b.Clicked() {
						actionConsumed = true
						it.OnPin()
					}
				}
			})
		}
	})

	// 点击选择：普通点击单选，平台修饰键加/减，Shift 选区间。
	if !actionConsumed && card.Clicked() {
		mods := card.ClickModifiers()
		switch {
		case mods&ui.Shift != 0:
			st.gridRange(st.grid.anchor, i)
		case mods&ui.Cmd != 0 || mods&ui.Ctrl != 0:
			if st.sel.Has(it.ID) {
				st.sel.Remove(it.ID)
			} else {
				st.sel.Add(it.ID)
			}
			st.grid.anchor = i
		default:
			st.sel.Clear()
			st.sel.Add(it.ID)
			st.grid.anchor = i
		}
		st.cur = i
		st.clicked, st.clickedB = it.ID, true
	}
	if !actionConsumed && card.DoubleClicked() {
		st.activated, st.activatedB = it.ID, true
	}
}

func (k Kit) gridA1Header(c *ui.Context, it GridItem, hideTrailing bool) {
	ui.Row(c.Key("head")).Padding(9, 11, 4, 11).Gap(6).AlignItems(ui.Center).Shrink(0).Children(func() {
		if it.SourceAppName != "" {
			ui.Row(c.Key("src-app")).Grow(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
				if it.SourceAppIcon != nil {
					ui.Image(c.Key("app-icon"), it.SourceAppIcon).Size(18, 18).Radius(4.5).Fit(ui.Contain).Shrink(0).AccessHidden(true)
				} else {
					ui.Box(c.Key("app-fallback")).Size(18, 18).Radius(4.5).Background(k.Color("background-tertiary-default")).Center().Shrink(0).AccessHidden(true).Children(func() {
						k.Icon(c, IconPackage, 11).TextColor(k.Color("text-secondary"))
					})
				}
				ui.Text(c.Key("app-name"), it.SourceAppName).FontSize(11.5).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine().MinWidth(0).Shrink(1)
				if it.KindLabel != "" {
					ui.Text(c.Key("kind-dot"), "· "+it.KindLabel).FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine().Shrink(0)
				}
			})
		} else {
			bg, fg := k.gridKindTint(it.Kind)
			label := it.KindLabel
			if label == "" {
				label = defaultKindLabel(it.Kind)
			}
			ui.Row(c.Key("kind-pill")).Height(20).PaddingX(6.5).Radius(6).Gap(4).AlignItems(ui.Center).Background(bg).Shrink(0).Children(func() {
				k.Icon(c, kindIcon(it.Kind), 11).TextColor(fg)
				ui.Text(c.Key("kl"), label).FontSize(10.5).FontWeight(600).TextColor(fg).SingleLine()
			})
			ui.Spacer(c)
		}
		if !hideTrailing {
			ui.Row(c.Key("trail")).Gap(5).AlignItems(ui.Center).Shrink(0).Children(func() {
				if it.Pinned {
					k.Icon(c.Key("pin"), IconPin, 11.5).TextColor(k.Color("status-orange-text"))
				}
				if it.Time != "" {
					ui.Text(c.Key("time"), it.Time).FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine()
				}
			})
		}
	})
}

func (k Kit) gridImageBody(c *ui.Context, it GridItem) {
	wrap := ui.Box(c.Key("img-wrap")).Grow(1).Basis(0).MinHeight(0).Clip().Background(k.Color(k.thumbToken()))
	wrap.Children(func() {
		if it.Media != nil {
			it.Media()
		} else {
			ui.Row(c).Grow(1).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
				k.Icon(c, IconImage, 24).TextColor(k.Color("text-secondary"))
			})
		}
		if it.SourceAppName != "" {
			pillBg := k.Color("background-primary-default").Alpha(.9)
			badge := ui.Row(c.Key("img-app")).Absolute().Top(8).Left(8).Padding(3, 8, 3, 4).Radius(8).Gap(5).
				AlignItems(ui.Center).Background(pillBg).ZIndex(2)
			k.shadowed(badge, "xs")
			badge.Children(func() {
				if it.SourceAppIcon != nil {
					ui.Image(c.Key("ico"), it.SourceAppIcon).Size(15, 15).Radius(3.5).Fit(ui.Contain).Shrink(0).AccessHidden(true)
				} else {
					k.Icon(c, IconPackage, 12).TextColor(k.Color("text-secondary"))
				}
				ui.Text(c.Key("nm"), it.SourceAppName).FontSize(10.5).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
			})
		}
		if it.Pinned {
			pinBg := k.Color("background-primary-default").Alpha(.9)
			ui.Box(c.Key("img-pin")).Absolute().Bottom(7).Right(7).Size(22, 22).Radius(6).Center().Background(pinBg).ZIndex(2).Children(func() {
				k.Icon(c, IconPin, 11).TextColor(k.Color("status-orange-text"))
			})
		}
	})
	ui.Row(c.Key("img-cap")).Padding(6, 11, 0, 11).Gap(6).AlignItems(ui.Baseline).Shrink(0).Children(func() {
		ui.Text(c.Key("title"), it.Title).FontSize(12.5).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine().Grow(1).MinWidth(0)
		if it.Time != "" {
			ui.Text(c.Key("time"), it.Time).FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine().Shrink(0)
		}
	})
}

func (k Kit) gridSemanticBody(c *ui.Context, it GridItem) {
	body := ui.Column(c.Key("body")).Grow(1).Basis(0).MinHeight(0).Padding(3, 11, 4, 11).Gap(4).Clip()
	body.Children(func() {
		switch it.Kind {
		case KindCode:
			ui.Box(c.Key("code")).Grow(1).MinHeight(0).Radius(8).Clip().Background(k.Color(k.codeBgToken())).Padding(8, 10).Children(func() {
				ui.Text(c, it.Text).Font("monospace").FontSize(11).TextColor(k.codeInk()).MaxLines(4)
			})
		case KindLink:
			host, path := splitLinkPreview(it.Text)
			linkCard := ui.Column(c.Key("link")).Grow(1).MinHeight(0).Radius(8).Clip().
				Background(k.Color("background-secondary-default")).Border(1, k.Color("separator-border")).
				Padding(8, 10).Justify(ui.SpaceBetween).Gap(3)
			linkCard.Children(func() {
				if host != "" {
					bg, fg := k.gridKindTint(KindLink)
					ui.Row(c.Key("host")).Gap(6).AlignItems(ui.Center).Children(func() {
						ui.Box(c.Key("fav")).Size(15, 15).Radius(4).Background(bg).Center().Shrink(0).Children(func() {
							k.Icon(c, IconLink, 9.5).TextColor(fg)
						})
						ui.Text(c.Key("h"), host).FontSize(11).FontWeight(600).TextColor(fg).SingleLine().MinWidth(0)
					})
				}
				if it.Title != "" && it.Title != it.Text {
					ui.Text(c.Key("t"), it.Title).FontSize(12.5).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
				}
				urlText := path
				if urlText == "" {
					urlText = it.Text
				}
				ui.Text(c.Key("u"), urlText).Font("monospace").FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine()
			})
		case KindFile, KindFolder:
			if it.Title != "" {
				ui.Text(c.Key("t"), it.Title).FontSize(12.5).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
			}
			names := nonEmptyLines(it.Text)
			if len(names) > 0 {
				ui.Column(c.Key("files")).Gap(4).Children(func() {
					limit := min(3, len(names))
					for idx := 0; idx < limit; idx++ {
						name := names[idx]
						ext, ebg, efg := k.fileExtBadge(name, it.Kind == KindFolder)
						ui.Row(c.Key(idx)).Padding(3, 7).Radius(6).Gap(6).AlignItems(ui.Center).
							Background(k.Color("background-secondary-default")).Children(func() {
							ui.Box(c.Key("ext")).Padding(1, 4).Radius(4).Background(ebg).Shrink(0).Children(func() {
								ui.Text(c, ext).FontSize(9).FontWeight(700).TextColor(efg).SingleLine()
							})
							ui.Text(c.Key("fn"), name).FontSize(11).TextColor(k.Color("text-primary")).SingleLine().MinWidth(0).Grow(1)
						})
					}
				})
			}
		default:
			trimmedTitle := strings.TrimSpace(it.Title)
			trimmedText := strings.TrimSpace(it.Text)
			isShort := trimmedText == "" || trimmedText == trimmedTitle ||
				(!strings.Contains(trimmedText, "\n") && len([]rune(trimmedText)) <= 56 && strings.HasPrefix(trimmedText, trimmedTitle))
			if isShort {
				display := trimmedText
				if display == "" {
					display = trimmedTitle
				}
				ui.Text(c.Key("short"), display).FontSize(13).FontWeight(500).TextColor(k.Color("text-primary")).MaxLines(4)
			} else {
				excerpt := stripLeadingTitle(trimmedText, trimmedTitle)
				if trimmedTitle != "" {
					ui.Text(c.Key("t"), trimmedTitle).FontSize(13).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
				}
				if excerpt != "" {
					ui.Text(c.Key("body-text"), excerpt).FontSize(12).TextColor(k.Color("text-secondary")).MaxLines(3)
				}
			}
		}
	})
}

func (k Kit) gridA1Footer(c *ui.Context, it GridItem) {
	meta := it.Meta
	if meta == "" {
		meta = it.Subtitle
	}
	ui.Row(c.Key("foot")).Padding(4, 11, 9, 11).Gap(6).AlignItems(ui.Center).Shrink(0).Children(func() {
		if meta != "" {
			ui.Text(c.Key("meta"), meta).FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine().Grow(1).MinWidth(0)
		} else {
			ui.Spacer(c)
		}
		if len(it.Tags) > 0 {
			ui.Row(c.Key("tags")).Gap(4).AlignItems(ui.Center).Shrink(0).Children(func() {
				limit := min(2, len(it.Tags))
				for idx := 0; idx < limit; idx++ {
					tag := it.Tags[idx]
					ui.Row(c.Key(idx)).Height(18).PaddingX(6).Radius(9).Gap(4).AlignItems(ui.Center).
						Background(k.Color("background-tertiary-default")).Children(func() {
						ui.Box(c.Key("dot")).Size(5.5, 5.5).Radius(3).Background(k.SwatchColor(tag.Color)).Shrink(0).AccessHidden(true)
						ui.Text(c.Key("lbl"), tag.Label).FontSize(10).FontWeight(500).TextColor(k.Color("text-secondary")).SingleLine()
					})
				}
			})
		}
	})
}

func (k Kit) gridKindTint(kind ContentKind) (bg, ink ui.Color) {
	switch kind {
	case KindCode:
		return k.TagTint("purple")
	case KindLink:
		return k.TagTint("blue")
	case KindImage:
		return k.TagTint("green")
	case KindFile:
		return k.TagTint("orange")
	case KindFolder:
		return k.TagTint("purple")
	default:
		return k.Color("background-tertiary-default"), k.Color("text-secondary")
	}
}

func (k Kit) fileExtBadge(name string, folder bool) (ext string, bg, fg ui.Color) {
	if folder {
		bg, fg = k.TagTint("purple")
		return "DIR", bg, fg
	}
	e := strings.TrimPrefix(strings.ToUpper(filepath.Ext(strings.TrimSpace(name))), ".")
	if e == "" {
		e = "FILE"
	} else if len(e) > 4 {
		e = e[:4]
	}
	switch e {
	case "PDF", "PPT", "PPTX":
		bg, fg = k.TagTint("orange")
	case "CSV", "XLS", "XLSX":
		bg, fg = k.TagTint("green")
	case "PNG", "JPG", "JPEG", "SVG", "WEBP":
		bg, fg = k.TagTint("blue")
	case "ZIP", "TAR", "GZ", "JSON":
		bg, fg = k.TagTint("purple")
	default:
		bg, fg = k.Color("background-tertiary-default"), k.Color("text-secondary")
	}
	return e, bg, fg
}

func defaultKindLabel(kind ContentKind) string {
	switch kind {
	case KindCode:
		return "Code"
	case KindLink:
		return "Link"
	case KindImage:
		return "Image"
	case KindFile:
		return "File"
	case KindFolder:
		return "Folder"
	default:
		return "Text"
	}
}

func splitLinkPreview(raw string) (host, path string) {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.Host == "" {
		return "", strings.TrimSpace(raw)
	}
	host = strings.TrimPrefix(u.Host, "www.")
	path = u.RequestURI()
	if path == "/" {
		path = ""
	}
	return host, path
}

func nonEmptyLines(s string) []string {
	var out []string
	for _, line := range strings.Split(s, "\n") {
		if t := strings.TrimSpace(line); t != "" {
			out = append(out, t)
		}
	}
	return out
}

func stripLeadingTitle(text, title string) string {
	if title == "" {
		return text
	}
	lines := strings.Split(text, "\n")
	if len(lines) > 1 && strings.TrimSpace(lines[0]) == title {
		return strings.TrimSpace(strings.Join(lines[1:], "\n"))
	}
	return text
}

func (k Kit) thumbToken() string {
	return "background-tertiary-default"
}

func (k Kit) codeBgToken() string {
	return "accent-950"
}

func (k Kit) codeInk() ui.Color { return k.Color("text-white") }

// gridRange 把选区设为 anchor..to（含）。
func (s *ListState) gridRange(from, to int) {
	if from < 0 || from >= len(s.grid.ids) {
		from = to
	}
	lo, hi := min(from, to), max(from, to)
	s.sel.Clear()
	for j := lo; j <= hi && j < len(s.grid.ids); j++ {
		s.sel.Add(s.grid.ids[j])
	}
}

// gridKeys 处理二维方向键、Home/End、全选、Enter、删除。
func (k Kit) gridKeys(e ui.Element, st *ListState, p GroupedGridProps, cols int) {
	n := len(p.Items)
	if n == 0 {
		return
	}
	if st.cur < 0 {
		st.cur = -1
	}
	move := func(to int, extend bool) {
		to = max(0, min(n-1, to))
		if extend {
			st.gridRange(st.grid.anchor, to)
		} else {
			st.sel.Clear()
			st.sel.Add(st.grid.ids[to])
			st.grid.anchor = to
		}
		st.cur = to
		st.list.ScrollIntoView(st.grid.rowOf[to])
	}
	for _, ext := range []bool{false, true} {
		var mods ui.Modifiers
		if ext {
			mods = ui.Shift
		}
		base := st.cur
		if base < 0 {
			base = 0
		}
		switch {
		case e.Shortcut(mods, ui.KeyRight):
			move(base+b2i(st.cur >= 0), ext)
		case e.Shortcut(mods, ui.KeyLeft):
			move(base-1, ext)
		case e.Shortcut(mods, ui.KeyDown):
			move(base+cols*b2i(st.cur >= 0), ext)
		case e.Shortcut(mods, ui.KeyUp):
			move(base-cols, ext)
		case e.Shortcut(mods, ui.KeyHome):
			move(0, ext)
		case e.Shortcut(mods, ui.KeyEnd):
			move(n-1, ext)
		}
	}
	if e.Shortcut(ui.Cmd, ui.KeyA) {
		st.sel.Clear()
		for _, id := range st.grid.ids {
			st.sel.Add(id)
		}
	}
	if e.Shortcut(0, ui.KeyEnter) && st.cur >= 0 && st.cur < n {
		st.activated, st.activatedB = st.grid.ids[st.cur], true
	}
	if k.deleteKey(e) {
		if sel := st.Selection(); len(sel) > 0 {
			st.deleted, st.deletedB = sel, true
		}
	}
}

func b2i(b bool) int {
	if b {
		return 1
	}
	return 0
}

// gridSettle 在网格构建后计算选区变化。
func (s *ListState) gridSettle(e ui.Element) {
	s.ids = s.grid.ids
	if now := s.selKey(); now != s.prevSel {
		s.prevSel, s.changed = now, true
	}
}

// visibleGrid 记录可见条目区间、翻页请求与“滚到末尾”边沿。
func (s *ListState) visibleGrid(n int, loaded [2]int, rows []ggRow, off int) {
	if n == 0 {
		s.first, s.last = 0, -1
		return
	}
	f, l := s.list.Visible()
	s.first, s.last = off, off
	for i := f; i <= l && i < n; i++ {
		if rows[i].first >= 0 {
			s.first = off + rows[i].first
			break
		}
	}
	for i := l; i >= f && i >= 0; i-- {
		if i < n && rows[i].first >= 0 {
			s.last = off + rows[i].first + rows[i].n - 1
			break
		}
	}
	if loaded[1] > loaded[0] {
		if f < loaded[0] {
			s.nav = NavNewer
		} else if l >= loaded[1] {
			s.nav = NavOlder
		}
	}
	near := l >= n-1-1
	s.endReached = near && !s.endWas
	s.endWas = near
}
