package desktopui

import (
	"bytes"
	"context"
	"fmt"
	"maps"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type tagEditorState struct {
	open         bool
	owner, query string
	highlight    int
	color        uikit.TagColor
}

var tagPalette = []uikit.TagColor{"orange", "blue", "green", "purple", "gray"}

func (d *viewState) openTagEditor() {
	if d.current() == nil || d.live != nil && d.live.busy {
		return
	}
	d.tagEditor = tagEditorState{open: true, owner: d.selected, color: "orange"}
}

// The shared registry owns names, counts and colors. The editor owns only one intent.
func (d *viewState) tagSuggestions(v *entry) []uikit.SearchSuggestion {
	name := strings.TrimSpace(d.tagEditor.query)
	needle := strings.ToLower(name)
	exists := false
	var similar, frequent []historyTag
	for _, tag := range d.tags {
		if strings.EqualFold(d.tagLabel(tag.id), name) || tag.id == name {
			exists = true
		}
		if tag.builtin || slices.Contains(v.tags, tag.id) {
			continue
		}
		frequent = append(frequent, tag)
		if needle != "" && strings.Contains(strings.ToLower(tag.name), needle) {
			similar = append(similar, tag)
		}
	}
	byUse := func(a, b historyTag) int { return d.tagCount(b.id) - d.tagCount(a.id) }
	candidates := similar
	limit := 6
	if len(similar) == 0 {
		candidates = frequent
		if needle != "" {
			limit = 3
		}
	}
	slices.SortStableFunc(candidates, func(a, b historyTag) int {
		if len(similar) > 0 {
			ap, bp := strings.HasPrefix(strings.ToLower(a.name), needle), strings.HasPrefix(strings.ToLower(b.name), needle)
			if ap != bp {
				if ap {
					return -1
				}
				return 1
			}
		}
		return byUse(a, b)
	})
	var out []uikit.SearchSuggestion
	if name != "" && !exists && !strings.HasPrefix(name, "builtin:") && utf8.RuneCountInString(name) <= 64 {
		out = append(out, uikit.SearchSuggestion{ID: name, Label: fmt.Sprintf(d.tr("新建 #%s"), name), Create: true})
	}
	for _, tag := range candidates[:min(limit, len(candidates))] {
		out = append(out, uikit.SearchSuggestion{ID: tag.id, Label: "#" + d.tagLabel(tag.id), Swatch: tag.color, Detail: fmt.Sprintf(d.tr("%d 项"), d.tagCount(tag.id))})
	}
	return out
}

func (d *viewState) detailTags(c *ui.Context, k uikit.Kit, v *entry) {
	if d.tagEditor.open && d.tagEditor.owner != v.id {
		d.tagEditor = tagEditorState{}
	}
	busy := d.live != nil && d.live.busy
	ui.Row(c).Gap(6).Wrap().AlignItems(ui.Center).Children(func() {
		for _, id := range v.tags {
			props := uikit.TagProps{Color: d.tagColor(id)}
			if tag := d.tagByID(id); !busy && tag != nil && !tag.builtin {
				props.RemoveLabel = fmt.Sprintf(d.tr("移除 #%s"), d.tagLabel(id))
			}
			if k.Tag(c.Key("tag:"+id), "#"+d.tagLabel(id), props).Removed {
				tags := slices.Clone(v.tags)
				tags = slices.DeleteFunc(tags, func(t string) bool { return t == id })
				d.setEntryTags(v.id, tags)
			}
		}
		if !d.tagEditor.open {
			if k.Button(c.Key("add"), d.tr("添加标签"), uikit.ButtonProps{Variant: uikit.Subtle, Icon: uikit.IconAdd, Size: uikit.SizeSmall, Disabled: busy}).Clicked() {
				d.openTagEditor()
			}
			return
		}
		editor := &d.tagEditor
		suggestions := d.tagSuggestions(v)
		if len(suggestions) > 0 {
			editor.highlight = max(0, min(editor.highlight, len(suggestions)-1))
		} else {
			editor.highlight = -1
		}
		picked := -1
		result := k.ComboField(c.Key("editor"), uikit.ComboFieldProps{
			Label: d.tr("标签名"), Prefix: "#", Value: &editor.query, Highlighted: &editor.highlight,
			Suggestions: suggestions, MaxLength: 64, AutoFocus: true,
			Footer:      d.tr("↑↓ 选择 · ↩ 添加 · ⇥ 颜色 · esc 关闭"),
			CreateColor: &uikit.ComboFieldCreateColor{Label: d.tr("颜色"), Colors: tagPalette, Value: &editor.color},
			OnKey: func(key uikit.SearchFieldKey) bool {
				switch key.Key {
				case ui.KeyDown, ui.KeyUp:
					if len(suggestions) > 0 {
						step := 1
						if key.Key == ui.KeyUp {
							step = -1
						}
						editor.highlight = (editor.highlight + step + len(suggestions)) % len(suggestions)
					}
					return true
				case ui.KeyEnter:
					if editor.highlight >= 0 && editor.highlight < len(suggestions) && !busy {
						d.pickTag(v, suggestions[editor.highlight], editor.color)
					}
					return true
				case ui.KeyTab:
					if editor.highlight >= 0 && suggestions[editor.highlight].Create {
						at := slices.Index(tagPalette, editor.color)
						step := 1
						if key.Shift() {
							step = -1
						}
						editor.color = tagPalette[(at+step+len(tagPalette))%len(tagPalette)]
						return true
					}
				case ui.KeyEscape:
					editor.open = false
					return true
				}
				return false
			},
		})
		if k.Skin == uikit.MacOS {
			result.Element.Width(170)
		}
		if result.Edited {
			editor.highlight = 0
		}
		if result.Selected >= 0 {
			picked = result.Selected
		}
		if picked >= 0 && picked < len(suggestions) && !busy {
			d.pickTag(v, suggestions[picked], editor.color)
		} else if result.Dismissed {
			editor.open = false
		}
	})
}

func (d *viewState) pickTag(v *entry, s uikit.SearchSuggestion, color uikit.TagColor) {
	if d.selected != v.id || d.live != nil && d.live.busy || slices.Contains(v.tags, s.ID) {
		return
	}
	tags := append(slices.Clone(v.tags), s.ID)
	if d.live == nil {
		if s.Create {
			d.tags = append(d.tags, historyTag{id: s.ID, name: s.ID, color: color, sidebar: true})
		}
		d.setEntryTags(v.id, tags)
	} else if !s.Create {
		d.setEntryTags(v.id, tags)
	} else {
		// A single user intent uses the existing config CAS and revision-checked history APIs.
		// A failed config save must never publish a color that was not persisted.
		cfg := d.live.config.Config
		cfg.Desktop.TagColors = maps.Clone(cfg.Desktop.TagColors)
		if cfg.Desktop.TagColors == nil {
			cfg.Desktop.TagColors = map[string]string{}
		}
		cfg.Desktop.TagColors[tagColorKey(s.ID)] = string(color)
		revision := d.live.config.Revision
		item, ok := d.live.items[v.id]
		if !ok {
			return
		}
		entryID := v.id
		client := d.live.client
		d.mutate(func(ctx context.Context) error {
			var buf bytes.Buffer
			if err := toml.NewEncoder(&buf).Encode(cfg); err != nil {
				return err
			}
			state, err := client.SaveConfig(ctx, revision, buf.Bytes())
			if err != nil {
				return err
			}
			if state.ApplyError != "" {
				return &api.Error{Reason: "config_apply_failed"}
			}
			_, err = client.UpdateHistory(ctx, entryID, api.HistoryUpdate{Revision: item.Revision, Tags: &tags})
			return err
		}, "标签已保存")
	}
	d.tagEditor.open = false
}
