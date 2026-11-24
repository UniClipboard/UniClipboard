package quickpanel

import (
	"context"
	"time"
)

type modifierReader interface {
	modifierSnapshot(string) func() (selected, other bool)
}

// WatchModifier observes only whether the selected modifier and any other key
// are down. No key identities leave the platform adapter.
func WatchModifier(ctx context.Context, platform Platform, modifier string, trigger func()) bool {
	reader, ok := platform.(modifierReader)
	if !ok || modifier == "" {
		return false
	}
	snapshot := reader.modifierSnapshot(modifier)
	if snapshot == nil {
		return false
	}
	go func() {
		ticker := time.NewTicker(20 * time.Millisecond)
		defer ticker.Stop()
		down, _ := snapshot()
		invalid := down
		var released time.Time
		for {
			select {
			case <-ctx.Done():
				return
			case now := <-ticker.C:
				selected, other := snapshot()
				if other {
					released = time.Time{}
				}
				if selected {
					invalid = other || (down && invalid)
				} else if down && !other && !invalid {
					if now.Sub(released) <= 400*time.Millisecond {
						released = time.Time{}
						if ctx.Err() == nil {
							trigger()
						}
					} else {
						released = now
					}
				}
				down = selected
			}
		}
	}()
	return true
}
