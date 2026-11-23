package main

import (
	"testing"

	"github.com/egoist/mygo"
)

type stubResizableWindow struct {
	maximized, fullScreen bool
	outer, content        mygo.Rectangle
	setBoundsCalls        []mygo.Rectangle
	setSizeCalls          [][2]int
}

func (w *stubResizableWindow) IsMaximized() bool             { return w.maximized }
func (w *stubResizableWindow) IsFullScreen() bool            { return w.fullScreen }
func (w *stubResizableWindow) Bounds() mygo.Rectangle        { return w.outer }
func (w *stubResizableWindow) ContentBounds() mygo.Rectangle { return w.content }
func (w *stubResizableWindow) SetContentBounds(r mygo.Rectangle) {
	w.setBoundsCalls = append(w.setBoundsCalls, r)
	insetLeft := w.content.X - w.outer.X
	insetTop := w.content.Y - w.outer.Y
	insetRight := (w.outer.X + w.outer.Width) - (w.content.X + w.content.Width)
	insetBottom := (w.outer.Y + w.outer.Height) - (w.content.Y + w.content.Height)
	w.content = r
	w.outer = mygo.Rectangle{
		X:      r.X - insetLeft,
		Y:      r.Y - insetTop,
		Width:  r.Width + insetLeft + insetRight,
		Height: r.Height + insetTop + insetBottom,
	}
}
func (w *stubResizableWindow) SetContentSize(width, height int) {
	w.setSizeCalls = append(w.setSizeCalls, [2]int{width, height})
	w.content.Width, w.content.Height = width, height
}

func TestCenteredContentBounds(t *testing.T) {
	workArea := mygo.Rectangle{X: 0, Y: 25, Width: 1440, Height: 875}

	tests := []struct {
		name     string
		outer    mygo.Rectangle
		content  mygo.Rectangle
		width    int
		height   int
		workArea mygo.Rectangle
		want     mygo.Rectangle
	}{
		{
			name:     "setup expands symmetrically in all directions",
			outer:    mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
			content:  mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: workArea,
			want:     mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
		},
		{
			name:     "main window shrinks symmetrically around center for later migration",
			outer:    mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
			content:  mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
			width:    780,
			height:   540,
			workArea: workArea,
			want:     mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
		},
		{
			name:     "clamps top-left inside work area",
			outer:    mygo.Rectangle{X: 20, Y: 40, Width: 780, Height: 540},
			content:  mygo.Rectangle{X: 20, Y: 40, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: workArea,
			want:     mygo.Rectangle{X: 0, Y: 25, Width: 1100, Height: 720},
		},
		{
			name:     "clamps bottom-right inside work area",
			outer:    mygo.Rectangle{X: 660, Y: 360, Width: 780, Height: 540},
			content:  mygo.Rectangle{X: 660, Y: 360, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: workArea,
			want:     mygo.Rectangle{X: 340, Y: 180, Width: 1100, Height: 720},
		},
		{
			name:     "accounts for system title bar and borders when clamping",
			outer:    mygo.Rectangle{X: 322, Y: 100, Width: 796, Height: 576},
			content:  mygo.Rectangle{X: 330, Y: 128, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: workArea,
			// Unclamped targetOuter.Y = (128 - 90) - 28 = 10 < 25, so outer.Y clamps to 25 and content.Y = 25 + 28 = 53.
			want: mygo.Rectangle{X: 170, Y: 53, Width: 1100, Height: 720},
		},
		{
			name:     "supports negative secondary display coordinates",
			outer:    mygo.Rectangle{X: -1350, Y: 270, Width: 780, Height: 540},
			content:  mygo.Rectangle{X: -1350, Y: 270, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: mygo.Rectangle{X: -1920, Y: 0, Width: 1920, Height: 1080},
			want:     mygo.Rectangle{X: -1510, Y: 180, Width: 1100, Height: 720},
		},
		{
			name:     "keeps top-left visible when display is smaller than target",
			outer:    mygo.Rectangle{X: 120, Y: 80, Width: 780, Height: 540},
			content:  mygo.Rectangle{X: 120, Y: 80, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: mygo.Rectangle{X: 0, Y: 25, Width: 1024, Height: 680},
			want:     mygo.Rectangle{X: 0, Y: 25, Width: 1100, Height: 720},
		},
		{
			name:     "expands centered without clamping when work area is empty",
			outer:    mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
			content:  mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: mygo.Rectangle{},
			want:     mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
		},
		{
			name:     "ignores invalid outer bounds safely",
			outer:    mygo.Rectangle{},
			content:  mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
			width:    1100,
			height:   720,
			workArea: workArea,
			want:     mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := centeredContentBounds(tc.outer, tc.content, tc.width, tc.height, tc.workArea)
			if got != tc.want {
				t.Fatalf("centeredContentBounds() = %+v, want %+v", got, tc.want)
			}
		})
	}
}

func TestResizeContentCenteredWith(t *testing.T) {
	workArea := mygo.Rectangle{X: 0, Y: 25, Width: 1440, Height: 875}
	lookup := func(mygo.Rectangle) mygo.Rectangle { return workArea }

	t.Run("expands setup window around its center and round-trips", func(t *testing.T) {
		win := &stubResizableWindow{
			outer:   mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
			content: mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540},
		}
		resizeContentCenteredWith(win, 1100, 720, lookup)
		wantExpanded := mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720}
		if len(win.setBoundsCalls) != 1 || win.setBoundsCalls[0] != wantExpanded {
			t.Fatalf("expanded bounds = %+v, want [%+v]", win.setBoundsCalls, wantExpanded)
		}

		resizeContentCenteredWith(win, 780, 540, lookup)
		wantSetup := mygo.Rectangle{X: 330, Y: 192, Width: 780, Height: 540}
		if len(win.setBoundsCalls) != 2 || win.setBoundsCalls[1] != wantSetup {
			t.Fatalf("restored setup bounds = %+v, want second call %+v", win.setBoundsCalls, wantSetup)
		}
	})

	t.Run("skips resize when maximized or fullscreen", func(t *testing.T) {
		for _, tc := range []struct {
			name                  string
			maximized, fullScreen bool
		}{
			{name: "maximized", maximized: true},
			{name: "fullscreen", fullScreen: true},
		} {
			win := &stubResizableWindow{
				maximized:  tc.maximized,
				fullScreen: tc.fullScreen,
				outer:      mygo.Rectangle{X: 0, Y: 25, Width: 1440, Height: 875},
				content:    mygo.Rectangle{X: 0, Y: 25, Width: 1440, Height: 875},
			}
			resizeContentCenteredWith(win, 1100, 720, lookup)
			if len(win.setBoundsCalls) != 0 || len(win.setSizeCalls) != 0 {
				t.Fatalf("%s: expected no resize, got bounds=%+v size=%+v", tc.name, win.setBoundsCalls, win.setSizeCalls)
			}
		}
	})

	t.Run("skips resize when already at target size", func(t *testing.T) {
		win := &stubResizableWindow{
			outer:   mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
			content: mygo.Rectangle{X: 170, Y: 102, Width: 1100, Height: 720},
		}
		resizeContentCenteredWith(win, 1100, 720, lookup)
		if len(win.setBoundsCalls) != 0 || len(win.setSizeCalls) != 0 {
			t.Fatalf("expected no-op resize, got bounds=%+v size=%+v", win.setBoundsCalls, win.setSizeCalls)
		}
	})

	t.Run("falls back to SetContentSize when current content size is empty", func(t *testing.T) {
		win := &stubResizableWindow{}
		resizeContentCenteredWith(win, 1100, 720, lookup)
		if len(win.setBoundsCalls) != 0 || len(win.setSizeCalls) != 1 || win.setSizeCalls[0] != [2]int{1100, 720} {
			t.Fatalf("expected SetContentSize fallback, got bounds=%+v size=%+v", win.setBoundsCalls, win.setSizeCalls)
		}
	})
}
