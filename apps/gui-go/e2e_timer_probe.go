//go:build e2e

package main

import (
	"log"
	"os"
	"time"
)

// startTimerProbe measures how late an ordinary Go timer fires in an idle process, the observable effect of
// App Nap timer coalescing (no unprivileged API reports whether a process is napping, so this is evidence of
// timer slowdown, not of the nap state itself). Every probeWindow it logs the tick count and the lateness of the
// 1s sleeps in it. It exists only in the e2e build and only when UC_GUI_GO_E2E_TIMER_PROBE=1.
func startTimerProbe() {
	if os.Getenv("UC_GUI_GO_E2E_TIMER_PROBE") != "1" {
		return
	}
	const (
		tick        = time.Second
		probeWindow = 30 * time.Second
	)
	go func() {
		var ticks int
		var maxLate, sumLate time.Duration
		windowStart := time.Now()
		for {
			want := time.Now().Add(tick)
			time.Sleep(tick)
			late := max(0, time.Since(want))
			ticks++
			sumLate += late
			maxLate = max(maxLate, late)
			if time.Since(windowStart) >= probeWindow {
				log.Printf("timer probe: window=%s ticks=%d max_late=%dms mean_late=%dms", time.Since(windowStart).Round(time.Second), ticks, maxLate.Milliseconds(), (sumLate / time.Duration(ticks)).Milliseconds())
				ticks, maxLate, sumLate, windowStart = 0, 0, 0, time.Now()
			}
		}
	}()
}
