package boardui_test

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

func TestToasterLifecycleAndControls(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}

	var store boardui.ToastStore
	var dismissed, acted, keptActed int
	busyAction := false

	view := func(c *ui.Context) {
		k := boardui.Kit{Dark: c.Theme().Dark}
		k.Apply(c)
		ui.Column(c).Fill().Background(k.Color("background-full")).Children(func() {
			k.Text(c.Key("page"), "Workspace", "body-regular")
		})
		k.Toaster(c, &store, boardui.ToasterProps{
			Position:   boardui.PositionBottomEnd,
			Limit:      3,
			Duration:   3 * time.Second,
			Label:      "Notifications",
			CloseLabel: "Dismiss notification",
		})
	}

	tt := ui.NewTesterWith(view, 900, 640, ui.TesterOptions{})

	// 1. Error toast uses RoleAlert; success/info/warning/neutral use RoleStatus.
	store.Error("Migration failed", boardui.ToastOptions{
		ID:          "mig-err",
		Description: "Please stop the standalone service and retry.",
		OnDismiss:   func() { dismissed++ },
	})
	tt.Frame()
	if !tt.HasText("Migration failed") || !tt.HasText("Please stop the standalone service and retry.") {
		t.Fatalf("error toast missing text: %v", tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleAlert, ""); !ok {
		t.Fatal("error toast must expose RoleAlert")
	}

	// Dismiss via CloseButton.
	if err := tt.Click("Dismiss notification"); err != nil {
		t.Fatalf("click close button: %v", err)
	}
	tt.Frame()
	if store.Len() != 0 || dismissed != 1 || tt.HasText("Migration failed") {
		t.Fatalf("toast did not dismiss cleanly: len=%d dismissed=%d texts=%v", store.Len(), dismissed, tt.Texts())
	}

	// 2. Action button runs callback and dismisses by default; KeepOpen keeps it alive.
	store.Warning("Restart required", boardui.ToastOptions{
		ID:       "restart",
		Duration: boardui.ToastPersistent,
		Actions: []boardui.ToastAction{
			{Label: "Restart now", KeepOpen: true, Busy: busyAction, Disabled: busyAction, OnClick: func() { keptActed++ }},
			{Label: "Later", Variant: "secondary", OnClick: func() { acted++ }},
		},
	})
	tt.Frame()
	if err := tt.Click("Restart now"); err != nil {
		t.Fatalf("click keep-open action: %v", err)
	}
	tt.Frame()
	if keptActed != 1 || !store.Has("restart") {
		t.Fatalf("KeepOpen action failed: keptActed=%d has=%v", keptActed, store.Has("restart"))
	}
	if err := tt.Click("Later"); err != nil {
		t.Fatalf("click dismissing action: %v", err)
	}
	tt.Frame()
	if acted != 1 || store.Has("restart") {
		t.Fatalf("default action did not dismiss toast: acted=%d has=%v", acted, store.Has("restart"))
	}

	// 3. Auto-dismiss countdown expires after duration, and pauses while hovered.
	store.Success("Changes saved", boardui.ToastOptions{ID: "save", Duration: 2 * time.Second})
	tt.Frame()
	bounds, ok := tt.Find("Changes saved")
	if !ok {
		t.Fatal("success toast not found")
	}
	// Hover over the toast card and advance past duration -> must stay open!
	tt.Move(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
	tt.Advance(2500 * time.Millisecond)
	if !store.Has("save") || !tt.HasText("Changes saved") {
		t.Fatal("hovered toast expired while paused")
	}
	// Move pointer away -> resumes countdown and dismisses after remaining duration.
	tt.Move(10, 10)
	tt.Advance(2500 * time.Millisecond)
	if store.Has("save") || tt.HasText("Changes saved") {
		t.Fatal("toast did not auto-dismiss after hover ended")
	}

	// 4. Limit enforcement drops oldest toasts first.
	for _, label := range []string{"Toast 1", "Toast 2", "Toast 3", "Toast 4"} {
		store.Info(label, boardui.ToastOptions{Duration: boardui.ToastPersistent})
	}
	tt.Frame()
	if store.Len() != 3 {
		t.Fatalf("limit not enforced: got %d want 3", store.Len())
	}
	if tt.HasText("Toast 1") || !tt.HasText("Toast 2") || !tt.HasText("Toast 3") || !tt.HasText("Toast 4") {
		t.Fatalf("unexpected visible toasts after limit: %v", tt.Texts())
	}

	// 5. Update and ID replacement modify in place without duplicating.
	id := store.Info("Exporting…", boardui.ToastOptions{ID: "exp", Loading: true, Duration: boardui.ToastPersistent, NonDismissible: true})
	tt.Frame()
	if !tt.HasText("Exporting…") {
		t.Fatal("loading toast missing")
	}
	store.Update(id, "Export ready", boardui.ToastOptions{Status: boardui.StatusSuccess, Duration: time.Second})
	tt.Frame()
	if !tt.HasText("Export ready") || tt.HasText("Exporting…") {
		t.Fatalf("updated toast did not replace title: %v", tt.Texts())
	}
	tt.Advance(1200 * time.Millisecond)
	if store.Has("exp") {
		t.Fatal("updated toast did not expire on new duration")
	}
}

func TestToasterDarkModeAndPositions(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	for _, dark := range []bool{false, true} {
		for _, pos := range []boardui.NotificationPosition{
			boardui.PositionTopStart,
			boardui.PositionTopCenter,
			boardui.PositionTopEnd,
			boardui.PositionBottomStart,
			boardui.PositionBottomCenter,
			boardui.PositionBottomEnd,
		} {
			var store boardui.ToastStore
			store.Show("Neutral", boardui.ToastOptions{Duration: boardui.ToastPersistent})
			store.Warning("Warning", boardui.ToastOptions{Duration: boardui.ToastPersistent})
			store.Error("Error", boardui.ToastOptions{Duration: boardui.ToastPersistent})
			tt := ui.NewTesterWith(func(c *ui.Context) {
				k := boardui.Kit{Dark: dark}
				k.Apply(c)
				ui.Box(c).Fill().Background(k.Color("background-full"))
				k.Toaster(c, &store, boardui.ToasterProps{Position: pos})
			}, 800, 600, ui.TesterOptions{Dark: dark})
			tt.Frame()
			if !tt.HasText("Neutral") || !tt.HasText("Warning") || !tt.HasText("Error") {
				t.Fatalf("dark=%v pos=%s missing toasts: %v", dark, pos, tt.Texts())
			}
		}
	}
}
