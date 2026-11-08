package host

import (
	"context"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/engine"
)

// pumpEvents turns the engine's events into the daemon's event stream. The
// engine's events only say what changed; this looks at what is different and
// emits the familiar peer-up, peer-down, pending, peers and code events.
// They carry names and kinds, never clipboard content.
func (n *Node) pumpEvents(ctx context.Context) {
	connected := map[engine.DeviceID]bool{}
	pending := map[engine.DeviceID]bool{}
	first := true
	for ctx.Err() == nil {
		sub, err := n.e.SubscribeEvents(ctx)
		if err != nil {
			select {
			case <-ctx.Done():
				return
			case <-time.After(time.Second):
			}
			continue
		}
		snap := sub.Snapshot()
		// What is already there is not news, except after a resync, where
		// differences since the last look are.
		n.diffDevices(snap.Devices, connected, pending, !first)
		first = false
		for {
			ev, err := sub.Next(ctx)
			if err != nil {
				break
			}
			switch ev.Kind {
			case engine.EventDevices:
				if ds, err := n.e.ListDevices(ctx); err == nil {
					n.diffDevices(ds, connected, pending, true)
				}
				for _, o := range ev.Objects {
					if o.Type == "channel" {
						n.hub.emit(api.Event{Kind: "channel", Info: o.ID})
					}
				}
			case engine.EventHistory:
				for _, o := range ev.Objects { // ids only: clients fetch the record again
					n.hub.emit(api.Event{Kind: "history", Info: o.ID})
				}
			case engine.EventPairingCode:
				n.hub.emit(api.Event{Kind: "code"})
			}
		}
		sub.Close()
	}
}

// diffDevices announces what changed in the device list since the last look.
func (n *Node) diffDevices(ds []engine.Device, connected, pending map[engine.DeviceID]bool, announce bool) {
	nowUp, nowPend := map[engine.DeviceID]bool{}, map[engine.DeviceID]bool{}
	for _, d := range ds {
		id := engine.DeviceID(d.ID)
		up, pend := devicePeer(d) && deviceConnected(d), devicePending(d)
		nowUp[id], nowPend[id] = up, pend
		if announce && up != connected[id] {
			kind := "peer-down"
			if up {
				kind = "peer-up"
			}
			n.hub.emit(api.Event{Kind: kind, Peer: d.DisplayName()})
		}
		if announce && pend && !pending[id] {
			info := ""
			for _, b := range d.Bindings {
				if b.Ticket {
					info = "code"
				}
			}
			n.hub.emit(api.Event{Kind: "pending", Peer: d.DisplayName(), Info: info})
		}
	}
	for id := range connected {
		delete(connected, id)
	}
	for id := range pending {
		delete(pending, id)
	}
	for id, v := range nowUp {
		connected[id] = v
	}
	for id, v := range nowPend {
		pending[id] = v
	}
	if announce {
		n.hub.emit(api.Event{Kind: "peers"}) // lists need a refresh
	}
}
