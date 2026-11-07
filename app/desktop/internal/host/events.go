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
		n.diffPeers(snap.Peers, connected, !first)
		n.diffPending(snap.Pending, pending, !first)
		first = false
		for {
			ev, err := sub.Next(ctx)
			if err != nil {
				break
			}
			switch ev.Kind {
			case engine.EventPeers:
				if ps, err := n.e.ListPeers(ctx); err == nil {
					n.diffPeers(ps, connected, true)
				}
			case engine.EventPending:
				if ps, err := n.e.ListPending(ctx); err == nil {
					n.diffPending(ps, pending, true)
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

func (n *Node) diffPeers(ps []engine.Peer, was map[engine.DeviceID]bool, announce bool) {
	now := map[engine.DeviceID]bool{}
	for _, p := range ps {
		up := p.Connection == engine.Connected
		now[p.ID] = up
		if announce && up != was[p.ID] {
			kind := "peer-down"
			if up {
				kind = "peer-up"
			}
			n.hub.emit(api.Event{Kind: kind, Peer: p.Name})
		}
	}
	for id := range was {
		delete(was, id)
	}
	for id, up := range now {
		was[id] = up
	}
	if announce {
		n.hub.emit(api.Event{Kind: "peers"})
	}
}

func (n *Node) diffPending(ps []engine.PendingPeer, was map[engine.DeviceID]bool, announce bool) {
	now := map[engine.DeviceID]bool{}
	for _, p := range ps {
		now[p.ID] = true
		if announce && !was[p.ID] {
			info := p.IntroducedByName
			if p.Source == engine.SourceCode {
				info = "code"
			}
			n.hub.emit(api.Event{Kind: "pending", Peer: p.Name, Info: info})
		}
	}
	removed := false
	for id := range was {
		removed = removed || !now[id]
		delete(was, id)
	}
	for id := range now {
		was[id] = true
	}
	if announce && removed {
		n.hub.emit(api.Event{Kind: "peers"}) // approved, rejected or withdrawn: lists need a refresh
	}
}
