package desktopui

import (
	"context"
	"fmt"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func testQuickEntries(n int, prefix string) []entry {
	out := make([]entry, n)
	for i := range n {
		id := fmt.Sprintf("%s-%02d", prefix, i)
		title := fmt.Sprintf("%s item %02d", prefix, i)
		out[i] = entry{
			id:     id,
			title:  title,
			text:   title,
			source: "local",
			date:   "刚刚",
		}
	}
	return out
}

func TestProcessPanelReopenResetsSelectionToFirstRow(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client := api.NewClient(filepath.Join(t.TempDir(), "unused.sock"))
	panel := NewProcessPanel(ctx, client, nil, func() config.Config { return config.Config{} }, nil)
	defer panel.Close()

	// First opening: populate 20 entries and select an entry outside the initial viewport.
	q1 := panel.newOpenState(false)
	panel.state = q1
	entries := testQuickEntries(20, "hist")
	q1.live.d.entries = entries
	q1.live.d.live.loaded = true
	q1.live.d.live.appliedSerial = 1
	q1.live.poll(q1)
	if q1.selected != 0 || q1.active() == nil || q1.active().id != "hist-00" {
		t.Fatalf("initial selection = (%d, %v), want (0, hist-00)", q1.selected, q1.active())
	}

	q1.query = "keep-query"
	q1.kind = "文本"
	q1.chips = []qpFilter{{dimension: "tag", value: "work", label: "#work"}}
	q1.windowZoom = 1
	q1.textZoom = 2
	q1.selectRow(15)
	if q1.selected != 15 || q1.active() == nil || q1.active().id != "hist-15" {
		t.Fatalf("selected row before close = (%d, %v), want (15, hist-15)", q1.selected, q1.active())
	}

	// Dismissing releases the view state without keeping the old selection identity.
	panel.release(q1, false)

	// Reopening preserves query/filters/zoom but selects the first visible result.
	q2 := panel.newOpenState(false)
	panel.state = q2
	defer panel.release(q2, false)

	if q2.query != "keep-query" || q2.kind != "文本" || len(q2.chips) != 1 || q2.chips[0].value != "work" {
		t.Fatalf("reopen did not preserve filters: query=%q kind=%q chips=%+v", q2.query, q2.kind, q2.chips)
	}
	if q2.windowZoom != 1 || q2.textZoom != 2 {
		t.Fatalf("reopen did not preserve zoom: window=%d text=%d", q2.windowZoom, q2.textZoom)
	}

	q2.live.d.entries = entries
	q2.live.d.live.loaded = true
	q2.live.d.live.appliedSerial = 1
	q2.live.poll(q2)

	if q2.selected != 0 || q2.active() == nil || q2.active().id != "hist-00" {
		t.Fatalf("reopen selection = (%d, %v), want (0, hist-00)", q2.selected, q2.active())
	}

	tt := ui.NewTesterWith(q2.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
	tt.Frame()
	row0, ok := tt.AccessByRole(ui.RoleButton, "历史：hist item 00")
	if !ok {
		t.Fatal("first row missing from accessibility tree")
	}
	const headerBottom = 48
	const footerTop = qpPanelHeight - 34
	if row0.Bounds.Y < headerBottom || row0.Bounds.Y+row0.Bounds.H > footerTop {
		t.Fatalf("first row bounds %+v outside visible viewport [%d, %d]", row0.Bounds, headerBottom, footerTop)
	}
}

func TestQuickPanelLiveApplySearchScrollsBackToFirstRow(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client := api.NewClient(filepath.Join(t.TempDir(), "unused.sock"))
	q := newLiveQuick(ctx, client, "light", nil)
	defer q.live.d.live.cancel()

	entries := testQuickEntries(25, "row")
	q.live.d.entries = entries
	q.live.d.live.loaded = true
	q.live.d.live.appliedSerial = 1
	q.live.poll(q)

	tt := ui.NewTesterWith(q.panel, qpPanelWidth, qpPanelHeight, ui.TesterOptions{Dark: false})
	tt.Frame()

	// Move selection deep into the list so row 0 scrolls above the viewport.
	q.selectRow(20)
	tt.Frame()
	tt.Frame()

	q.gridTop = 2
	q.applySearch()
	if q.selected != 0 || q.gridTop != 0 || !q.revealSelection || !q.keyboard {
		t.Fatalf("applySearch state = (selected=%d gridTop=%d reveal=%v keyboard=%v), want (0, 0, true, true)", q.selected, q.gridTop, q.revealSelection, q.keyboard)
	}

	// Simulate fresh search results arriving on the next poll.
	q.live.d.entries = entries
	q.live.d.live.appliedSerial = 2
	q.live.poll(q)
	tt.Frame()
	tt.Frame()

	if q.selected != 0 {
		t.Fatalf("selected after search results = %d, want 0", q.selected)
	}
	row0, ok := tt.AccessByRole(ui.RoleButton, "历史：row item 00")
	if !ok {
		t.Fatal("row 0 missing after applySearch")
	}
	const headerBottom = 48
	const footerTop = qpPanelHeight - 34
	if row0.Bounds.Y < headerBottom || row0.Bounds.Y+row0.Bounds.H > footerTop {
		t.Fatalf("row 0 bounds %+v not scrolled back into viewport [%d, %d]", row0.Bounds, headerBottom, footerTop)
	}
}
