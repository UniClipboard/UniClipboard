package desktopui

import (
	"runtime"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/plugin"
)

func TestSearchMatcherMultilingualAndPinyin(t *testing.T) {
	t.Run("builtin-prefix-does-not-pollute", func(t *testing.T) {
		// Typing "#b" must NOT match builtin:files just because its internal ID starts with "builtin:".
		if _, ok := matchSearchCandidate("tag", plugin.TagFiles, "文件", "文件", "b"); ok {
			t.Fatal("builtin: internal prefix leaked into #b match for builtin:files")
		}
		if _, ok := matchSearchCandidate("tag", plugin.TagFiles, "Files", "文件", "u"); ok {
			t.Fatal("builtin: internal prefix leaked into #u match for builtin:files")
		}
		// Typing "#f" still matches builtin:files via canonical key/alias "files"/"file".
		if _, ok := matchSearchCandidate("tag", plugin.TagFiles, "文件", "文件", "f"); !ok {
			t.Fatal("#f should match builtin:files")
		}
	})

	t.Run("chinese-label-and-pinyin-initials", func(t *testing.T) {
		d := newDemo("history", "dark")
		d.tags = append(builtinTags(),
			historyTag{id: "项目文档", name: "项目文档", color: "blue"},
			historyTag{id: "银行账单", name: "银行账单", color: "green"},
		)

		// #文 -> matches #文件 / #文件夹 / #文本
		cands := d.searchCandidates("tag", "文")
		ids := make([]string, len(cands))
		for i, c := range cands {
			ids[i] = c.value
		}
		if !slices.Contains(ids, plugin.TagFiles) || !slices.Contains(ids, plugin.TagFolder) || !slices.Contains(ids, plugin.TagText) {
			t.Fatalf("#文 missing expected builtin tags: %v", ids)
		}

		// #wj -> matches #文件 ranked ahead of #文件夹
		cands = d.searchCandidates("tag", "wj")
		ids = make([]string, len(cands))
		for i, c := range cands {
			ids[i] = c.value
		}
		fileIdx := slices.Index(ids, plugin.TagFiles)
		folderIdx := slices.Index(ids, plugin.TagFolder)
		if fileIdx < 0 || folderIdx < 0 || fileIdx > folderIdx {
			t.Fatalf("#wj expected builtin:files before builtin:folder, got %v", ids)
		}

		// #wjj -> matches #文件夹
		cands = d.searchCandidates("tag", "wjj")
		if len(cands) == 0 || cands[0].value != plugin.TagFolder {
			t.Fatalf("#wjj expected builtin:folder first, got %+v", cands)
		}

		// Custom Chinese tags via pinyin initials, full pinyin, and later segment:
		for _, q := range []string{"xm", "xmwd", "xiangmu", "xiangm", "wd"} {
			cands = d.searchCandidates("tag", q)
			if !slices.ContainsFunc(cands, func(c searchCandidate) bool { return c.value == "项目文档" }) {
				t.Fatalf("#%s did not match custom Chinese tag 项目文档: %+v", q, cands)
			}
		}

		// Heteronym support: 银行 (hang/xing) -> yh
		cands = d.searchCandidates("tag", "yh")
		if !slices.ContainsFunc(cands, func(c searchCandidate) bool { return c.value == "银行账单" }) {
			t.Fatalf("#yh did not match heteronym custom tag 银行账单: %+v", cands)
		}

		// Custom Chinese device name via pinyin initials:
		cands = d.searchCandidates("source", "gzz")
		if !slices.ContainsFunc(cands, func(c searchCandidate) bool { return c.value == "linux" }) {
			t.Fatalf("@gzz did not match Linux 工作站: %+v", cands)
		}
	})

	t.Run("multilingual-localized-and-diacritic-folding", func(t *testing.T) {
		// Japanese: /画像 matches type:image, /fairu matches type:file
		d := newDemo("history", "dark")
		d.language = "ja"
		cands := d.searchCandidates("type", "画像")
		if len(cands) == 0 || cands[0].value != "image" {
			t.Fatalf("ja /画像 expected image first, got %+v", cands)
		}
		cands = d.searchCandidates("type", "fairu")
		if len(cands) == 0 || cands[0].value != "file" {
			t.Fatalf("ja /fairu expected file first, got %+v", cands)
		}

		// Portuguese: diacritic folding (#codigo matches builtin:code)
		d.language = "pt-BR"
		d.tags = builtinTags()
		cands = d.searchCandidates("tag", "codigo")
		if len(cands) == 0 || cands[0].value != plugin.TagCode {
			t.Fatalf("pt-BR #codigo expected builtin:code first, got %+v", cands)
		}

		// Russian: /файл matches type:file
		d.language = "ru"
		cands = d.searchCandidates("type", "файл")
		if len(cands) == 0 || cands[0].value != "file" {
			t.Fatalf("ru /файл expected file first, got %+v", cands)
		}
	})
}

func TestGUISearchFavoritesQuickSwitch(t *testing.T) {
	d := newDemo("history", "dark")
	tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true})
	total := len(d.filtered())
	favCount := 0
	for _, e := range d.entries {
		if e.pinned {
			favCount++
		}
	}
	if favCount == 0 || favCount >= total {
		t.Fatalf("unexpected fixture favorite count: fav=%d total=%d", favCount, total)
	}

	// 1. Switch to Favorites via #sc pinyin suggestion in search box.
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	tt.Type("#sc")
	if _, ok := tt.AccessByRole(ui.RoleOption, "★ 已收藏"); !ok {
		t.Fatal("#sc did not suggest ★ 已收藏")
	}
	tt.Key(0, ui.KeyEnter)
	if !d.search.favorite {
		t.Fatal("Enter did not enable favorite search filter")
	}
	if slices.Contains(selectedSearchTags(d.tag), plugin.TagFavorite) {
		t.Fatal("reserved favorite tag leaked into d.tag")
	}
	if len(d.filtered()) != favCount {
		t.Fatalf("favorite filter count=%d want=%d", len(d.filtered()), favCount)
	}
	// Backspace removes the favorite chip cleanly.
	tt.Key(0, ui.KeyBackspace)
	if d.search.favorite || len(d.filtered()) != total {
		t.Fatalf("Backspace did not clear favorite filter: fav=%v count=%d", d.search.favorite, len(d.filtered()))
	}

	// 2. Switch to Favorites via FilterPopover chip.
	if err := tt.Click("筛选条件"); err != nil {
		t.Fatal(err)
	}
	if err := tt.Click("★ 仅看已收藏"); err != nil {
		t.Fatal(err)
	}
	if !d.search.favorite || len(d.filtered()) != favCount {
		t.Fatalf("filter popover favorite toggle failed: fav=%v count=%d", d.search.favorite, len(d.filtered()))
	}
}

func TestQuickPanelFavoritesAndFullKeyboardUX(t *testing.T) {
	command := ui.Cmd
	if runtime.GOOS != "darwin" {
		command = ui.Ctrl
	}

	create := func() (*quickPanelState, *ui.Tester) {
		p := newQuickDemo("dark", "history")
		return p, ui.NewTesterWith(p.panel, 380, 366, ui.TesterOptions{Dark: true})
	}

	t.Run("favorites-shortcut-and-and-semantics", func(t *testing.T) {
		p, tt := create()
		total := len(p.rows)
		tt.Key(command, ui.KeyF)
		if !p.favoriteOnly {
			t.Fatal("Cmd/Ctrl+F did not toggle favoriteOnly")
		}
		if len(p.rows) == 0 || len(p.rows) >= total {
			t.Fatalf("expected filtered favorites subset, got %d of %d", len(p.rows), total)
		}
		for _, r := range p.rows {
			if !r.favorite {
				t.Fatalf("non-favorite row leaked into favorites view: %+v", r)
			}
		}

		// Combine favoriteOnly with a tag: must use AND semantics.
		tt.Type("#工作")
		tt.Key(0, ui.KeyEnter)
		tt.Advance(310 * time.Millisecond)
		for _, r := range p.rows {
			if !r.favorite || !slices.Contains(r.tags, "工作") {
				t.Fatalf("favorites + #工作 violated AND semantics: %+v", r)
			}
		}
		// Verify Engine query also uses TagMatch=all and includes favorite.
		q := p.searchQuery()
		if q.TagMatch != "all" || !slices.Contains(q.Tags, plugin.TagFavorite) || !slices.Contains(q.Tags, "工作") {
			t.Fatalf("unexpected live searchQuery: %+v", q)
		}

		// Pressing Cmd/Ctrl+F again turns favorites filter off while preserving #工作 chip.
		tt.Key(command, ui.KeyF)
		if p.favoriteOnly {
			t.Fatal("second Cmd/Ctrl+F did not disable favoriteOnly")
		}
		if len(p.chips) != 1 || p.chips[0].value != "工作" {
			t.Fatalf("toggling favorites lost existing chips: %+v", p.chips)
		}
	})

	t.Run("explicit-prefix-suggestions-default-highlight-and-pinyin", func(t *testing.T) {
		p, tt := create()
		// Plain literal words must NOT pop up suggestions.
		tt.Type("code")
		if len(p.suggestions()) != 0 {
			t.Fatalf("plain text 'code' unexpectedly triggered suggestions: %+v", p.suggestions())
		}
		tt.Key(0, ui.KeyEscape)

		// Explicit #wj triggers suggestions with #文件 highlighted by default.
		tt.Type("#wj")
		opts := p.suggestions()
		if len(opts) < 2 || opts[0].value != "文件" || !p.suggestFocused || p.suggestCursor != 0 {
			t.Fatalf("#wj expected default-highlighted 文件 first, got focused=%v cursor=%d opts=%+v", p.suggestFocused, p.suggestCursor, opts)
		}
		// Down arrow moves inside suggestions (not history list!) to #文件夹.
		tt.Key(0, ui.KeyDown)
		if p.suggestCursor != 1 || p.selected != 0 {
			t.Fatalf("Down did not move within suggestions: suggestCursor=%d selected=%d", p.suggestCursor, p.selected)
		}
		// Shift+Tab moves back to previous suggestion without switching content type.
		tt.Key(ui.Shift, ui.KeyTab)
		if p.suggestCursor != 0 || p.kind != "" {
			t.Fatalf("Shift+Tab did not move back in suggestions: suggestCursor=%d kind=%q", p.suggestCursor, p.kind)
		}
		// Enter accepts the highlighted suggestion (#文件) instead of pasting history.
		tt.Key(0, ui.KeyEnter)
		if len(p.effects) != 0 || len(p.chips) != 1 || p.chips[0].value != "文件" || p.query != "" {
			t.Fatalf("Enter did not accept suggestion cleanly: effects=%+v chips=%+v query=%q", p.effects, p.chips, p.query)
		}

		// #sc accepts Favorites filter.
		tt.Type("#sc")
		tt.Key(0, ui.KeyEnter)
		if !p.favoriteOnly || p.query != "" {
			t.Fatalf("#sc + Enter did not enable favoriteOnly: fav=%v query=%q", p.favoriteOnly, p.query)
		}
	})

	t.Run("menu-locks-text-input-and-arrow-submenu", func(t *testing.T) {
		p, tt := create()
		tt.Key(command, ui.KeyK)
		if p.menu != "main" {
			t.Fatal("menu did not open")
		}
		// Typing stray letters must NOT mutate query or close menu.
		tt.Type("xyz")
		if p.menu != "main" || p.query != "" {
			t.Fatalf("typing leaked through open action menu: menu=%q query=%q", p.menu, p.query)
		}
		// Navigate to "发送到设备" and open with Right arrow, go back with Left arrow.
		for i := 0; i < len(p.actions()); i++ {
			if p.actions()[p.cursor].id == "devices" {
				break
			}
			tt.Key(0, ui.KeyDown)
		}
		tt.Key(0, ui.KeyRight)
		if p.menu != "devices" {
			t.Fatalf("Right arrow did not enter devices submenu: menu=%q", p.menu)
		}
		tt.Key(0, ui.KeyLeft)
		if p.menu != "main" {
			t.Fatalf("Left arrow did not return to main menu: menu=%q", p.menu)
		}
		tt.Key(0, ui.KeyEscape)
		if p.menu != "" {
			t.Fatal("Escape did not close menu")
		}
	})

	t.Run("paging-preview-toggle-scroll-image-zoom-and-relaxation-shortcut", func(t *testing.T) {
		p, tt := create()
		// Immediate preview toggle via Ctrl+Space without 500ms wait.
		if p.previewID != "" {
			t.Fatal("preview should start closed")
		}
		tt.Key(ui.Ctrl, ui.KeySpace)
		if p.previewID != p.rows[0].id {
			t.Fatalf("Ctrl+Space did not open preview immediately: previewID=%q", p.previewID)
		}
		tt.Key(ui.Ctrl, ui.KeySpace)
		if p.previewID != "" {
			t.Fatal("second Ctrl+Space did not close preview")
		}

		// PageDown / End / Home navigation.
		tt.Key(0, ui.KeyPageDown)
		if p.selected == 0 {
			t.Fatal("PageDown did not advance selection")
		}
		tt.Key(0, ui.KeyEnd)
		if p.selected != len(p.rows)-1 {
			t.Fatalf("End did not jump to last row: selected=%d len=%d", p.selected, len(p.rows))
		}
		tt.Key(0, ui.KeyHome)
		if p.selected != 0 {
			t.Fatalf("Home did not jump to first row: selected=%d", p.selected)
		}

		// Image preview 1:1 toggle via Cmd/Ctrl+0.
		tt.Type("/图片")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Key(ui.Ctrl, ui.KeySpace)
		tt.Key(command, ui.Key0)
		if !p.actualSize {
			t.Fatal("Cmd/Ctrl+0 did not toggle image actualSize")
		}
		tt.Key(command, ui.Key0)
		if p.actualSize {
			t.Fatal("second Cmd/Ctrl+0 did not restore fit mode")
		}

		// Alt+Right steps in image grid even when query is non-empty.
		p.query = "图"
		tt.Frame()
		tt.Key(ui.Alt, ui.KeyRight)
		if p.selected != 1 {
			t.Fatalf("Alt+Right did not move in image grid with non-empty query: selected=%d", p.selected)
		}

		// Relaxation numeric shortcut Cmd/Ctrl+1 on empty result.
		p.clear()
		tt.Advance(310 * time.Millisecond)
		tt.Type("/图片")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Type("#工作")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) != 0 || len(p.relaxations()) == 0 {
			t.Fatal("expected zero rows with relaxation proposals")
		}
		tt.Key(command, ui.Key1)
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) == 0 {
			t.Fatal("Cmd/Ctrl+1 did not apply first relaxation proposal")
		}
	})
}
