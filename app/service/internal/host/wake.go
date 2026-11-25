package host

import (
	"context"
	"sync"
	"time"

	"tailscale.com/net/netmon"
	"tailscale.com/util/eventbus"

	api "github.com/uniclipboard/unic/internal/localapi"
)

const (
	wakeDebounceDelay     = 1500 * time.Millisecond
	wakeMinGap            = 3 * time.Second
	networkChangeTimeout  = 15 * time.Second
	refreshRequestTimeout = 10 * time.Second
)

type wakeState struct {
	mu      sync.Mutex
	timer   *time.Timer
	lastRun time.Time
	stopped bool
}

func (w *wakeState) stop() {
	w.mu.Lock()
	w.stopped = true
	if w.timer != nil {
		w.timer.Stop()
		w.timer = nil
	}
	w.mu.Unlock()
}

// ScheduleNetworkChanged coalesces rapid host wake and interface-change signals
// so Wi-Fi/DHCP settles briefly and duplicate notifications trigger one warmup.
func (n *Node) ScheduleNetworkChanged() {
	n.wake.mu.Lock()
	if n.wake.stopped {
		n.wake.mu.Unlock()
		return
	}
	if n.wake.timer != nil {
		n.wake.timer.Reset(wakeDebounceDelay)
		n.wake.mu.Unlock()
		return
	}
	delay := wakeDebounceDelay
	if since := time.Since(n.wake.lastRun); since < wakeMinGap {
		delay = max(delay, wakeMinGap-since)
	}
	n.wake.timer = time.AfterFunc(delay, func() {
		n.wake.mu.Lock()
		if n.wake.stopped {
			n.wake.mu.Unlock()
			return
		}
		n.wake.timer = nil
		n.wake.lastRun = time.Now()
		n.wake.mu.Unlock()
		ctx, cancel := context.WithTimeout(n.runCtx, networkChangeTimeout)
		defer cancel()
		_ = n.e.NotifyNetworkChanged(ctx)
	})
	n.wake.mu.Unlock()
}

// NotifyNetworkChanged triggers a coalesced wake/network re-evaluation.
func (n *Node) NotifyNetworkChanged(ctx context.Context) error {
	if ctx.Err() != nil {
		return ctx.Err()
	}
	n.ScheduleNetworkChanged()
	return nil
}

// RefreshConnections runs an explicit bounded connection check for one device
// (or all devices when deviceID is empty) and returns the updated daemon status.
func (n *Node) RefreshConnections(ctx context.Context, deviceID string) (api.Status, error) {
	rctx, cancel := context.WithTimeout(ctx, refreshRequestTimeout)
	defer cancel()
	if _, err := n.e.RefreshConnections(rctx, deviceID); err != nil {
		return api.Status{}, err
	}
	return n.Status(ctx)
}

// watchHostWake monitors OS interface changes and wall-clock jumps (sleep/wake)
// without any UI dependencies so headless unicd also heals connections on wake.
func (n *Node) watchHostWake(ctx context.Context) {
	bus := eventbus.New()
	mon, err := netmon.New(bus, func(string, ...any) {})
	if err != nil {
		bus.Close()
		return
	}
	ec := bus.Client("unic-wake")
	eventbus.SubscribeFunc(ec, func(delta netmon.ChangeDelta) {
		if delta.TimeJumped() || delta.RebindLikelyRequired {
			n.ScheduleNetworkChanged()
		}
	})
	mon.Start()
	<-ctx.Done()
	_ = mon.Close()
	ec.Close()
	bus.Close()
}
